//! Actions from a terminal. Keys and choices work on a node's folder. The rest need the core, so
//! it starts without a window for as long as the action takes.
use super::commands::{Command, Personal};
use super::console::{answer_to, looked_up};
use super::folder::Folder;
use super::host_locks::plain_detail;
use super::ops::Book;
use super::service::{Core, CoreLaunch};
use super::snapshot::{BoardView, Notice};
use super::words::Desk;
use super::{new_coin, write_coin_file};
use crate::fleet::{FleetPort, RealFleet};
use crate::instance::{claim, Instance};
use crate::settings::{standard_folder, Paths, Settings};
use olc_board::carried::firmware_path;
use olc_board::control_key::{config_folder, ControlKey};
use olc_board::install::install;
use olc_control::BoardId;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

/// Time for boards to show up after the core starts.
const BOARDS_APPEAR: Duration = Duration::from_secs(8);
/// How long the list stays the same before it counts as whole.
const BOARDS_SETTLE: Duration = Duration::from_secs(4);
/// Time for a stored chain to load.
const CHAIN_LOADS: Duration = Duration::from_secs(120);
/// Time for a node nearby to answer.
const PEERS_APPEAR: Duration = Duration::from_secs(15);
/// A payment is on its way out before this process ends.
const LEAVING: Duration = Duration::from_secs(2);
/// A board that takes this long with no change has stopped.
const UPDATE_STALLS: Duration = Duration::from_secs(600);
const LOOK_EVERY: Duration = Duration::from_millis(100);
const NOBODY_NEAR: &str = "No node answered, so nothing was sent.";
const OTHER_WALLET: &str =
    "That needs a node on the wallet in --data. Run olc node --data DIR and type it there.";
/// Where `olc` kept a node's folder before it followed the core's.
const OLD_FOLDER: &str = ".olc/node";

/// The same folder, however it was written.
fn same_folder(one: &Path, other: &Path) -> bool {
    let whole = |path: &Path| path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    whole(one) == whole(other)
}

pub struct Terminal {
    pub folder: Folder,
    /// The app's folder, for the actions that need the core.
    pub root: Option<PathBuf>,
    /// Ports a person named. Empty finds every board by its USB id.
    pub ports: Vec<String>,
    pub firmware: Option<String>,
    /// Asked for before anything runs, so this stays free of prompts.
    pub given_password: Option<String>,
    /// The core, with the hold that keeps a second copy off the same wallet and cables.
    core: Option<(Core, Instance)>,
}

impl Terminal {
    pub fn new(folder: Folder) -> Self {
        Self {
            folder,
            root: None,
            ports: Vec::new(),
            firmware: None,
            given_password: None,
            core: None,
        }
    }

    /// The wallet every word of a run reads: `data` when given, else the wallet of the core in
    /// `root` or in the standard folder. A one-shot word and `olc node` with the same flags meet
    /// on it, and so does a word that starts the core.
    pub fn wallet_folder(data: Option<&str>, root: Option<&Path>) -> Result<PathBuf, String> {
        if let Some(data) = data {
            return Ok(PathBuf::from(data));
        }
        let root = match root {
            Some(root) => root.to_path_buf(),
            None => standard_folder()?,
        };
        Ok(Paths { root }.wallet())
    }

    /// A line for a person whose wallet sits where `olc` kept it before, beside the one in use.
    pub fn old_wallet_here(chosen: &Path) -> Option<String> {
        let old = Path::new(OLD_FOLDER);
        let there = old.join("wallet.json").is_file() && !same_folder(old, chosen);
        there.then(|| format!("A wallet is also in {OLD_FOLDER} here. Add --data {OLD_FOLDER} to use it."))
    }

    fn core(&mut self) -> Result<&Core, String> {
        if self.core.is_none() {
            let root = match &self.root {
                Some(root) => root.clone(),
                None => standard_folder()?,
            };
            let instance = claim(&root)?.ok_or("OLC Core is open. Use it, or close it first.")?;
            let fleet: Arc<dyn FleetPort> = Arc::new(RealFleet::new(self.ports.clone())?);
            let settings = Settings::load(&root);
            let launch = CoreLaunch {
                paths: Paths { root },
                fleet: Some(fleet),
                usb: true,
                node_program: None,
            };
            self.core = Some((Core::start(launch, settings)?, instance));
        }
        let core = self.core.as_ref().map(|(core, _)| core);
        core.ok_or_else(|| "The core did not start.".to_string())
    }

    /// Whether the core's wallet and chain are the ones the rest of the run reads. An order on
    /// them goes nowhere else.
    fn same_wallet(&self, command: &Command) -> Result<(), String> {
        if !command.needs_the_chain() {
            return Ok(());
        }
        let root = match &self.root {
            Some(root) => root.clone(),
            None => standard_folder()?,
        };
        match same_folder(&Paths { root }.wallet(), &self.folder.directory) {
            true => Ok(()),
            false => Err(OTHER_WALLET.to_string()),
        }
    }

    /// The core, once it holds its chain.
    fn loaded_core(&mut self) -> Result<&Core, String> {
        let core = self.core()?;
        wait_for(CHAIN_LOADS, || core.snapshot().node.coin.is_some());
        Ok(core)
    }

    /// Hands `command` to the core and waits for what it says.
    fn through_core(&mut self, command: Command) -> Result<String, String> {
        let core = self.loaded_core()?;
        answer_to(&|| core.snapshot(), &|one| core.send(one), command)
    }

    /// This process ends after the command, so what it sends needs someone to take it first.
    fn send_and_stay(&mut self, command: Command) -> Result<String, String> {
        let core = self.loaded_core()?;
        if !wait_for(PEERS_APPEAR, || core.snapshot().node.peers > 0) {
            return Err(NOBODY_NEAR.to_string());
        }
        let answer = answer_to(&|| core.snapshot(), &|one| core.send(one), command)?;
        thread::sleep(LEAVING);
        Ok(answer)
    }

    /// Stays until the board is back, printing each stage, since ending here would end the update.
    fn update_board(&mut self, id: BoardId) -> Result<String, String> {
        let core = self.core()?;
        core.send(Command::UpdateBoard(id));
        let job_now = || {
            let snapshot = core.snapshot();
            let row = snapshot.boards.iter().find(|view| view.seen.id == id);
            row.and_then(|view| view.job.clone())
        };
        if !wait_for(BOARDS_APPEAR, || job_now().is_some()) {
            return Err("The update did not start.".to_string());
        }
        let (mut shown, mut changed) = (None, Instant::now());
        while changed.elapsed() < UPDATE_STALLS {
            let job = job_now();
            if job != shown {
                changed = Instant::now();
                shown.clone_from(&job);
                job.iter().for_each(|stage| println!("{stage}"));
            }
            match job.as_deref() {
                None => return Ok("Updated.".to_string()),
                Some(failed @ ("update_failed" | "unproven")) => return Err(failed.to_string()),
                Some(_) => thread::sleep(LOOK_EVERY),
            }
        }
        Err("The board stopped answering.".to_string())
    }
}

impl Drop for Terminal {
    /// Orders that give no notice are only sent. The core takes each before this process ends.
    fn drop(&mut self) {
        if let Some((core, _)) = &self.core {
            core.finish();
        }
    }
}

/// Looks until `ready` says yes or the time is up. Says which came first.
fn wait_for(time: Duration, ready: impl Fn() -> bool) -> bool {
    let started = Instant::now();
    while !ready() {
        if started.elapsed() >= time {
            return false;
        }
        thread::sleep(LOOK_EVERY);
    }
    true
}

impl Desk for Terminal {
    fn command(&mut self, command: Command) -> Result<String, String> {
        let on_folder = match &command {
            Command::Own(Personal::Explore(_)) => false,
            Command::Own(order) => !order.needs_a_node(),
            _ => false,
        };
        if !on_folder {
            self.same_wallet(&command)?;
        }
        match command {
            Command::Own(Personal::Explore(text)) => Ok(looked_up(self.loaded_core()?, text)),
            Command::Own(order) if order.goes_out() => self.send_and_stay(Command::Own(order)),
            Command::Own(order) if order.needs_a_node() => self.through_core(Command::Own(order)),
            Command::Own(order) => match self.folder.carry_out(order) {
                Ok((key, detail)) => Ok(Notice { number: 0, key, detail, good: true }.sentence()),
                Err(why) => Err(plain_detail(&why).to_string()),
            },
            // A coin put under a lock or spent from one leaves like any payment, or not at all.
            Command::Pay { .. } | Command::Lock(_) => self.send_and_stay(command),
            Command::UpdateBoard(id) => self.update_board(id),
            Command::NewCoinFile { choices, path } => {
                let coin = new_coin(&choices).map_err(|bad| bad.to_string())?;
                write_coin_file(&coin, &path)?;
                Ok(Notice { number: 0, key: "notice.coin_saved", detail: path, good: true }.sentence())
            }
            Command::InstallBoard { path, name } => {
                let key = ControlKey::load_or_create(&config_folder()?)?.public()?;
                install(&path, &name, &firmware_path(self.firmware.as_deref()), key)?;
                Ok(name)
            }
            command => self.through_core(command),
        }
    }

    fn boards(&mut self) -> Result<Vec<BoardView>, String> {
        let core = self.core()?;
        let started = Instant::now();
        let (mut seen, mut grew) = (0, Instant::now());
        loop {
            let boards = core.snapshot().boards.clone();
            if boards.len() != seen {
                (seen, grew) = (boards.len(), Instant::now());
            }
            // The first to answer is rarely the one asked for: a radio's beacon beats a cable's.
            let settled = seen > 0 && grew.elapsed() >= BOARDS_SETTLE;
            if settled || started.elapsed() >= BOARDS_APPEAR {
                return Ok(boards);
            }
            thread::sleep(LOOK_EVERY);
        }
    }

    fn password(&mut self) -> Result<String, String> {
        self.given_password.take().ok_or_else(|| "No password was given.".to_string())
    }
}
