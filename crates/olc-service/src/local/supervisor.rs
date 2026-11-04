//! The app's side of its node processes: starting and stopping them, reading their status lines
//! and measuring them from the operating system.
use super::measure::{Measured, Meter};
use super::spawn::{limits_of, memory_limit_bytes, Enforcement};
use super::{NodeSpec, WalletChoice};
use crate::core::{NodeView, Verdict};
use crate::settings::Paths;
use olc_platform::{platform, Hold};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Stdio};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

const FIRST_PORT: u16 = 7100;
const QUIT_WAIT: Duration = Duration::from_secs(2);
const MIB: u64 = 1024 * 1024;
const KEPT_ERROR_LINES: usize = 8;

/// One status line of a node process.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ChildStatus {
    pub height: u64,
    pub tip: String,
    pub work: String,
    pub hashrate: u64,
    pub cap: u64,
    pub mining: bool,
    pub peers: usize,
    pub pool: usize,
    pub balance: u64,
    pub spendable: u64,
    pub blocks_mined: u64,
    pub address: String,
    pub port: Option<u16>,
    pub stored_bytes: u64,
    pub storage_full: bool,
    /// Headers held, on a node that holds nothing else.
    pub headers: Option<u64>,
    /// Proofs and headers that did not pass.
    pub refused: u32,
    pub reorganized: u32,
    pub paid: Option<String>,
    /// Why an order failed, as the key of a sentence, and what goes into it.
    pub error: Option<String>,
    pub detail: String,
    /// A spend under a lock that waits for another signer.
    pub half: Option<String>,
    /// The public key of the node's wallet, for a lock that names it.
    pub public: String,
    /// Headers found on work from outside since the last line.
    #[serde(default)]
    pub shares: Vec<String>,
    /// Keeping to itself, as a board does when told `offline on`. Said in an experiment only.
    pub offline: bool,
    /// The newest blocks of its chain. Said in an experiment only.
    pub chain: Vec<ChainBlock>,
    /// What a node of the Lab's network does beside being a node. Said in the Lab only.
    pub lab: Option<super::lab_side::LabLine>,
    /// What it last said of blocks and payments it was given, oldest first.
    pub verdicts: Vec<Verdict>,
}

/// One block as an experiment draws it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ChainBlock {
    pub height: u64,
    pub id: String,
    /// The address its reward went to.
    pub miner: String,
    /// Its payments, without the reward.
    pub txs: Vec<String>,
}

impl ChildStatus {
    pub fn from_view(view: &NodeView) -> Self {
        Self {
            height: view.height,
            tip: view.tip_hex(),
            work: view.work.to_string(),
            hashrate: view.hashrate,
            cap: view.cap,
            mining: view.mining,
            peers: view.peers,
            pool: view.pool,
            balance: view.balance,
            spendable: view.spendable,
            blocks_mined: view.blocks_mined,
            address: view.address.clone(),
            port: view.port,
            stored_bytes: view.stored_bytes,
            storage_full: view.storage_full,
            headers: view.light.map(|light| light.held_headers as u64),
            refused: view.light.map_or(0, |light| light.refused),
            reorganized: view.light.map_or(0, |light| light.reorganized),
            paid: None,
            error: None,
            detail: String::new(),
            half: None,
            public: view.keys.first().map_or_else(String::new, |key| key.public.clone()),
            shares: Vec::new(),
            offline: false,
            chain: Vec::new(),
            lab: None,
            verdicts: view.verdicts.clone(),
        }
    }
}

/// One node process as the screen sees it.
#[derive(Clone, Debug)]
pub struct LocalView {
    pub spec: NodeSpec,
    pub pid: Option<u32>,
    pub status: Option<ChildStatus>,
    pub measured: Option<Measured>,
    pub memory_limit: u64,
    pub enforcement: Enforcement,
    /// How the last run ended, if it did not end by request.
    pub ended: Option<String>,
}

/// Board lines a node process printed go here while an experiment listens.
type BoardLines = Arc<Mutex<Option<Sender<String>>>>;

struct Running {
    child: Child,
    input: Arc<Mutex<ChildStdin>>,
    board_lines: BoardLines,
    status: Arc<Mutex<Option<ChildStatus>>>,
    errors: JoinHandle<VecDeque<String>>,
    meter: Meter,
    _hold: Option<Hold>,
}

pub struct Supervisor {
    paths: Paths,
    program: PathBuf,
    /// The threads every node started from here mines on: the app's thread cap.
    threads: usize,
    running: BTreeMap<u32, Running>,
    ended: BTreeMap<u32, String>,
    answers: Arc<Mutex<Vec<(u32, ChildStatus)>>>,
}

fn port_of(id: u32) -> u16 {
    FIRST_PORT + (id as u16 % 800)
}

/// Keeps the last lines a node wrote to its error stream, so a node that ended can say why.
/// The pipe is always drained, so a node never stops on a full pipe.
fn read_errors(child: &mut Child) -> Result<JoinHandle<VecDeque<String>>, String> {
    let errors = child.stderr.take().ok_or("The node gave no error stream.")?;
    Ok(thread::spawn(move || {
        let mut kept = VecDeque::new();
        for line in BufReader::new(errors).lines().map_while(Result::ok) {
            if kept.len() == KEPT_ERROR_LINES {
                kept.pop_front();
            }
            kept.push_back(line);
        }
        kept
    }))
}

/// The exit as the system tells it, followed by what the node last said.
fn how_it_ended(exit: &str, said: VecDeque<String>) -> String {
    let said = Vec::from(said).join(" | ");
    if said.is_empty() { exit.to_string() } else { format!("{exit}: {said}") }
}

fn read_status(
    id: u32,
    child: &mut Child,
    status: Arc<Mutex<Option<ChildStatus>>>,
    answers: Arc<Mutex<Vec<(u32, ChildStatus)>>>,
    board_lines: BoardLines,
) {
    let Some(output) = child.stdout.take() else {
        return;
    };
    thread::spawn(move || {
        for line in BufReader::new(output).lines().map_while(Result::ok) {
            if !line.starts_with('{') {
                if let Some(sender) = board_lines.lock().ok().as_ref().and_then(|slot| slot.as_ref()) {
                    let _ = sender.send(line);
                }
                continue;
            }
            let Ok(parsed) = serde_json::from_str::<ChildStatus>(&line) else {
                continue;
            };
            let answer = parsed.paid.is_some() || parsed.error.is_some() || parsed.half.is_some();
            if answer || !parsed.shares.is_empty() {
                if let Ok(mut answers) = answers.lock() {
                    answers.push((id, parsed.clone()));
                }
            }
            if let Ok(mut slot) = status.lock() {
                *slot = Some(parsed);
            }
        }
    });
}

impl Supervisor {
    pub fn new(paths: Paths) -> Result<Self, String> {
        let program = std::env::current_exe().map_err(|error| error.to_string())?;
        Ok(Self::with_program(paths, program))
    }

    /// With the program to run as a node, which a test names itself.
    pub fn with_program(paths: Paths, program: PathBuf) -> Self {
        Self {
            paths,
            program,
            threads: 1,
            running: BTreeMap::new(),
            ended: BTreeMap::new(),
            answers: Arc::default(),
        }
    }

    fn wallet_folder(&self, spec: &NodeSpec) -> PathBuf {
        match spec.wallet {
            WalletChoice::Own => self.paths.local_node(spec.id).join("wallet"),
            WalletChoice::Shared => self.paths.wallet(),
        }
    }

    fn words(&self, spec: &NodeSpec, coin: &Path, chain: &Path, peers: &[String]) -> Vec<String> {
        let mut words: Vec<String> = vec![
            "node".into(),
            "--data".into(),
            chain.display().to_string(),
            "--wallet".into(),
            self.wallet_folder(spec).display().to_string(),
            "--coin".into(),
            coin.display().to_string(),
            "--bind".into(),
            if spec.listen_lan { "0.0.0.0" } else { "127.0.0.1" }.into(),
            "--port".into(),
            port_of(spec.id).to_string(),
            "--cap".into(),
            spec.hash_cap.to_string(),
            "--storage".into(),
            (spec.disk_mib * MIB).to_string(),
            "--name".into(),
            spec.name.clone(),
        ];
        for peer in peers {
            words.extend(["--connect".to_string(), peer.clone()]);
        }
        if self.threads > 1 {
            words.extend(["--threads".to_string(), self.threads.to_string()]);
        }
        if spec.mining {
            words.push("--mine".into());
        }
        words
    }

    /// The threads nodes started from now on mine on. Running ones are told with `threads N`.
    pub fn set_threads(&mut self, threads: usize) {
        self.threads = threads.max(1);
    }

    /// Starts a node on the coin file at `coin`, connected to `peers`.
    pub fn start(&mut self, spec: &NodeSpec, coin: &Path, peers: &[String]) -> Result<(), String> {
        if self.running.contains_key(&spec.id) {
            return Ok(());
        }
        let folder = self.paths.local_node(spec.id);
        let coin_name = coin.parent().and_then(Path::file_name).unwrap_or_default();
        let chain = folder.join(coin_name);
        fs::create_dir_all(&chain).map_err(|error| error.to_string())?;
        spec.apply_holding(&chain)?;
        let words = self.words(spec, coin, &chain, peers);
        let how = platform().enforcement();
        let limits = limits_of(spec);
        let program = self.program.display().to_string();
        let mut command = platform().limited(how, &limits, &program, &words);
        platform().hide_window(&mut command);
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| error.to_string())?;
        let hold = platform().hold(how, &limits, &child);
        let input = child.stdin.take().ok_or("The node took no input.")?;
        let errors = read_errors(&mut child)?;
        let status = Arc::new(Mutex::new(None));
        let board_lines = BoardLines::default();
        read_status(spec.id, &mut child, status.clone(), self.answers.clone(), board_lines.clone());
        let meter = Meter::new(child.id());
        self.ended.remove(&spec.id);
        self.running.insert(
            spec.id,
            Running {
                child,
                input: Arc::new(Mutex::new(input)),
                board_lines,
                status,
                errors,
                meter,
                _hold: hold,
            },
        );
        Ok(())
    }

    pub fn stop(&mut self, id: u32) {
        let Some(mut node) = self.running.remove(&id) else {
            return;
        };
        if let Ok(mut input) = node.input.lock() {
            let _ = writeln!(input, "quit");
        }
        // Reaps it away from the screen's path.
        thread::spawn(move || {
            thread::sleep(QUIT_WAIT);
            if matches!(node.child.try_wait(), Ok(None)) {
                let _ = node.child.kill();
            }
            let _ = node.child.wait();
        });
    }

    pub fn stop_all(&mut self) {
        let ids: Vec<u32> = self.running.keys().copied().collect();
        ids.into_iter().for_each(|id| self.stop(id));
    }

    pub fn send(&mut self, id: u32, line: &str) -> Result<(), String> {
        let node = self.running.get_mut(&id).ok_or("not_running")?;
        let mut input = node.input.lock().map_err(|_| "not_running".to_string())?;
        writeln!(input, "{line}").map_err(|error| error.to_string())
    }

    /// The node as a board an experiment can drive: lines go in as `board <line>`, and the
    /// answers come back as a board prints them. A second link takes over from the first.
    pub fn link(&self, id: u32) -> Option<LocalLink> {
        let node = self.running.get(&id)?;
        let (sender, lines) = channel();
        *node.board_lines.lock().ok()? = Some(sender);
        Some(LocalLink { input: node.input.clone(), lines })
    }

    /// The last status line of a node, without measuring it.
    pub fn status(&self, id: u32) -> Option<ChildStatus> {
        self.running.get(&id)?.status.lock().ok()?.clone()
    }

    pub fn pid(&self, id: u32) -> Option<u32> {
        self.running.get(&id).map(|node| node.child.id())
    }

    /// The answers to payments since the last call.
    pub fn take_answers(&self) -> Vec<(u32, ChildStatus)> {
        self.answers
            .lock()
            .map(|mut answers| answers.drain(..).collect())
            .unwrap_or_default()
    }

    /// Notices processes that ended, as a kill would.
    pub fn reap(&mut self) {
        let mut gone = Vec::new();
        for (id, node) in &mut self.running {
            if let Ok(Some(exit)) = node.child.try_wait() {
                gone.push((*id, exit.to_string()));
            }
        }
        for (id, exit) in gone {
            let Some(node) = self.running.remove(&id) else {
                continue;
            };
            // The process is gone, so its pipe has closed and this returns at once.
            let said = node.errors.join().unwrap_or_default();
            self.ended.insert(id, how_it_ended(&exit, said));
        }
    }

    pub fn view(&mut self, spec: &NodeSpec) -> LocalView {
        let node = self.running.get_mut(&spec.id);
        let (pid, status, measured) = match node {
            Some(node) => {
                let status = node.status.lock().ok().and_then(|status| status.clone());
                let disk = status.as_ref().map_or(0, |status| status.stored_bytes);
                (Some(node.child.id()), status, Some(node.meter.read(disk)))
            }
            None => (None, None, None),
        };
        LocalView {
            spec: spec.clone(),
            pid,
            status,
            measured,
            memory_limit: memory_limit_bytes(spec),
            enforcement: platform().enforcement(),
            ended: self.ended.get(&spec.id).cloned(),
        }
    }
}

impl Drop for Supervisor {
    fn drop(&mut self) {
        for node in self.running.values_mut() {
            if let Ok(mut input) = node.input.lock() {
                let _ = writeln!(input, "quit");
            }
        }
    }
}

/// A node process driven with a board's lines. See [`Supervisor::link`].
pub struct LocalLink {
    input: Arc<Mutex<ChildStdin>>,
    lines: Receiver<String>,
}

impl olc_board::BoardLink for LocalLink {
    fn send(&mut self, line: &str) -> Result<(), String> {
        let mut input = self.input.lock().map_err(|_| "The node stopped.".to_string())?;
        for part in line.lines().filter(|part| !part.trim().is_empty()) {
            writeln!(input, "board {part}").map_err(|_| "The node stopped.".to_string())?;
        }
        Ok(())
    }

    fn receive(&mut self, wait: Duration) -> Option<String> {
        self.lines.recv_timeout(wait).ok()
    }
}

/// Where the node of `spec` listens, for those connecting to it.
pub fn address_of(spec: &NodeSpec) -> String {
    format!("127.0.0.1:{}", port_of(spec.id))
}
