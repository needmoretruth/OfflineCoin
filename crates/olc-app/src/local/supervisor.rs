//! The app's side of its node processes: starting and stopping them, reading their status lines
//! and measuring them from the operating system.
use super::measure::{Measured, Meter};
use super::spawn::{enforcement, memory_limit_bytes, wrapped, Enforcement};
use super::{NodeSpec, WalletChoice};
use crate::core::NodeView;
use crate::settings::Paths;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

const FIRST_PORT: u16 = 7100;
const QUIT_WAIT: Duration = Duration::from_secs(2);
const MIB: u64 = 1024 * 1024;

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
    pub loading: bool,
    pub paid: Option<String>,
    pub error: Option<String>,
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
            loading: view.loading.is_some(),
            paid: None,
            error: None,
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

struct Running {
    child: Child,
    input: ChildStdin,
    status: Arc<Mutex<Option<ChildStatus>>>,
    meter: Meter,
}

pub struct Supervisor {
    paths: Paths,
    program: PathBuf,
    running: BTreeMap<u32, Running>,
    ended: BTreeMap<u32, String>,
    answers: Arc<Mutex<Vec<(u32, ChildStatus)>>>,
}

fn port_of(id: u32) -> u16 {
    FIRST_PORT + (id as u16 % 800)
}

fn read_status(
    id: u32,
    child: &mut Child,
    status: Arc<Mutex<Option<ChildStatus>>>,
    answers: Arc<Mutex<Vec<(u32, ChildStatus)>>>,
) {
    let Some(output) = child.stdout.take() else {
        return;
    };
    thread::spawn(move || {
        for line in BufReader::new(output).lines().map_while(Result::ok) {
            let Ok(parsed) = serde_json::from_str::<ChildStatus>(&line) else {
                continue;
            };
            if parsed.paid.is_some() || parsed.error.is_some() {
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
        if spec.mining {
            words.push("--mine".into());
        }
        words
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
        let words = self.words(spec, coin, &chain, peers);
        let mut command = wrapped(
            enforcement(),
            spec,
            &self.program.display().to_string(),
            &words,
        );
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| error.to_string())?;
        let input = child.stdin.take().ok_or("The node took no input.")?;
        let status = Arc::new(Mutex::new(None));
        read_status(spec.id, &mut child, status.clone(), self.answers.clone());
        let meter = Meter::new(child.id());
        self.ended.remove(&spec.id);
        self.running.insert(
            spec.id,
            Running {
                child,
                input,
                status,
                meter,
            },
        );
        Ok(())
    }

    pub fn stop(&mut self, id: u32) {
        let Some(mut node) = self.running.remove(&id) else {
            return;
        };
        let _ = writeln!(node.input, "quit");
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
        writeln!(node.input, "{line}").map_err(|error| error.to_string())
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
        for (id, text) in gone {
            self.running.remove(&id);
            self.ended.insert(id, text);
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
            enforcement: enforcement(),
            ended: self.ended.get(&spec.id).cloned(),
        }
    }
}

impl Drop for Supervisor {
    fn drop(&mut self) {
        for node in self.running.values_mut() {
            let _ = writeln!(node.input, "quit");
        }
    }
}

/// Where the node of `spec` listens, for those connecting to it.
pub fn address_of(spec: &NodeSpec) -> String {
    format!("127.0.0.1:{}", port_of(spec.id))
}
