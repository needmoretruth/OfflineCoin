//! What a program that manages boards needs, whatever its face: the boards on the cables, the
//! fleet they reach, and the jobs a person starts. Runs next to a [`Runner`], and holds no screen.
use crate::clearance::{deliver, Ledger, PORT_FOLDER, UNPROVEN};
use crate::control_key::{config_folder, ControlKey};
use crate::install::{app_image, install as install_board};
use crate::journal::{Entry, Journal};
use crate::pins::Pins;
use crate::remote::{command, hash_of};
use crate::rom;
use crate::{Args, BoardLink, Connection, Extra, Fleet, RemoteBoard, Say, Seen, SharedFleet};
use olc_control::{allowed_remotely, BoardId, Forward};
use olc_link_serial::SerialLink;
use olc_node::{Message, Outgoing, Runner};
use olc_wallet::format_address;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const PORT_MARK: &str = "Espressif";
const WATCH_EVERY: Duration = Duration::from_secs(2);
/// A port stays quiet this long before a person is offered to install on it.
const NEW_BOARD_AFTER: Duration = Duration::from_secs(14);
const MAX_LOG: usize = 400;
const MAX_LOADER_WAKES: u32 = 3;
const RETURN_WAIT: Duration = Duration::from_secs(600);

struct Port {
    link: usize,
    opened: Instant,
    closer: Arc<AtomicBool>,
    device: u64,
}

/// A run of an experiment or other optional program.
#[derive(Default)]
pub struct ExtraRun {
    pub lines: VecDeque<String>,
    pub running: bool,
    pub stop: Option<Arc<AtomicBool>>,
}

/// Why a line sent to a board did not run.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum LineError {
    BadRequest,
    NotDone,
    Refused,
    NoAnswer,
}

/// Why an experiment did not start.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum StartError {
    Busy,
}

/// One board as a person sees it.
pub struct BoardRow {
    pub seen: Seen,
    /// `None` for a board heard through another, or over a network link.
    pub on_cable: bool,
    /// The name of the board this one is reached through.
    pub through: Option<String>,
    pub job: Option<String>,
}

pub struct Manager {
    pub fleet: SharedFleet,
    pub firmware: PathBuf,
    /// Given by hand. Without any, the manager finds boards by itself.
    fixed_ports: Vec<String>,
    ports: Mutex<BTreeMap<String, Port>>,
    busy_ports: Mutex<BTreeSet<String>>,
    loader_wakes: Mutex<BTreeMap<String, u32>>,
    /// What is happening to a board or a port.
    jobs: Mutex<BTreeMap<String, String>>,
    pub run: Arc<Mutex<ExtraRun>>,
}

/// `link_of` tells which link a peer came in on, as [`Runner::link_of`] does.
impl Manager {
    pub fn new(firmware: PathBuf, fixed_ports: Vec<String>) -> Result<Arc<Self>, String> {
        let folder = config_folder()?;
        let key = ControlKey::load_or_create(&folder)?;
        let mut fleet = Fleet::new(key, Runner::link_of)?;
        fleet.keep_pins(Pins::stored_in(folder));
        Ok(Arc::new(Self {
            fleet: Arc::new(Mutex::new(fleet)),
            firmware,
            fixed_ports,
            ports: Mutex::new(BTreeMap::new()),
            busy_ports: Mutex::new(BTreeSet::new()),
            loader_wakes: Mutex::new(BTreeMap::new()),
            jobs: Mutex::new(BTreeMap::new()),
            run: Arc::new(Mutex::new(ExtraRun::default())),
        }))
    }

    /// Keeps the list of ports current. Call `pump` from the loop that steps the runner.
    pub fn watch_ports(self: &Arc<Self>, runner: Arc<Mutex<Runner>>) {
        let manager = self.clone();
        thread::spawn(move || loop {
            manager.watch(&runner);
            manager.put_boards_back();
            thread::sleep(WATCH_EVERY);
        });
    }

    /// Undoes what a run that died left on the boards, as soon as each one can be reached.
    fn put_boards_back(&self) {
        let running = self.run.lock().map_or(true, |run| run.running);
        let Ok(journal) = Journal::standard() else {
            return;
        };
        if running {
            return;
        }
        for entry in journal.pending() {
            if entry.is_held_elsewhere() {
                continue;
            }
            let rows = self.board_rows();
            let row = rows.iter().find(|row| {
                row.seen.online && format_address(&row.seen.beacon.address) == entry.address
            });
            if let Some(row) = row {
                if self.put_back(row, &entry) {
                    journal.forget(&entry.address);
                }
            }
        }
    }

    fn put_back(&self, row: &BoardRow, entry: &Entry) -> bool {
        let done = entry
            .lines
            .iter()
            .all(|line| self.run_line(row.seen.id, line).is_ok());
        if done {
            let line = format!(
                "{} is back to its state before a run that died.",
                row.seen.beacon.name
            );
            eprintln!("{line}");
            push_line(&self.run, line);
        }
        done
    }

    /// Moves control packets between the links and the fleet.
    pub fn pump(&self, runner: &mut Runner) {
        let Ok(mut fleet) = self.fleet.lock() else {
            return;
        };
        while let Some((peer, bytes)) = runner.take_control() {
            fleet.hear(peer, &bytes);
        }
        while let Some((forward, bytes)) = fleet.take_outgoing() {
            let message = Message::Control(bytes);
            runner.send_now(match forward {
                Forward::Nowhere => continue,
                Forward::Peer(peer) => Outgoing::To(peer, message),
                Forward::All => Outgoing::All {
                    except: None,
                    message,
                },
                Forward::AllExcept(peer) => Outgoing::All {
                    except: Some(peer),
                    message,
                },
            });
        }
    }

    pub fn set_job(&self, key: &str, text: Option<String>) {
        if let Ok(mut jobs) = self.jobs.lock() {
            match text {
                Some(text) => jobs.insert(key.to_string(), text),
                None => jobs.remove(key),
            };
        }
    }

    /// A board is taking an update right now.
    pub fn is_updating(&self) -> bool {
        self.jobs.lock().is_ok_and(|jobs| {
            jobs.values()
                .any(|job| job != "update_failed" && job != "installing")
        })
    }

    /// Something is running that an update must not cut into.
    pub fn is_busy(&self) -> bool {
        self.is_updating() || self.run.lock().map_or(true, |run| run.running)
    }

    pub fn job(&self, key: &str) -> Option<String> {
        self.jobs.lock().ok()?.get(key).cloned()
    }

    fn present_ports(&self) -> Vec<String> {
        if !self.fixed_ports.is_empty() {
            return self.fixed_ports.clone();
        }
        let Ok(entries) = fs::read_dir(PORT_FOLDER) else {
            return Vec::new();
        };
        entries
            .filter_map(Result::ok)
            .map(|entry| entry.path().display().to_string())
            .filter(|path| path.contains(PORT_MARK))
            .collect()
    }

    /// A board that came up in the chip's loader stays dark until something restarts it. It
    /// comes back on the next look. A board that keeps landing there is left alone.
    fn wake_from_loader(&self, path: &str) -> bool {
        let Ok(mut wakes) = self.loader_wakes.lock() else {
            return false;
        };
        let tried = wakes.entry(path.to_string()).or_insert(0);
        if *tried >= MAX_LOADER_WAKES || !rom::wake(path) {
            return false;
        }
        *tried += 1;
        true
    }

    /// Opens what appeared and lets go of what left.
    fn watch(&self, runner: &Mutex<Runner>) {
        let present = self.present_ports();
        let busy = self
            .busy_ports
            .lock()
            .map(|set| set.clone())
            .unwrap_or_default();
        let Ok(mut ports) = self.ports.lock() else {
            return;
        };
        ports.retain(|path, port| {
            // A board that left and came back between two looks has a new device behind the
            // same name, and the file open on the old one hears nothing any more.
            let same = device_of(path) == Some(port.device);
            let keep = same && present.contains(path) && !busy.contains(path);
            if !keep {
                port.closer.store(true, Ordering::Relaxed);
            }
            keep
        });
        for path in present {
            if ports.contains_key(&path) || busy.contains(&path) {
                continue;
            }
            let Some(device) = device_of(&path) else {
                continue;
            };
            if self.wake_from_loader(&path) {
                continue;
            }
            let Ok(link) = SerialLink::open(&path, |_| {}) else {
                continue;
            };
            let closer = link.closer();
            if let Ok(mut runner) = runner.lock() {
                let link = runner.add_link(Box::new(link));
                let opened = Instant::now();
                ports.insert(
                    path,
                    Port {
                        link,
                        opened,
                        closer,
                        device,
                    },
                );
            }
        }
    }

    /// Ports whose board is not yet one of ours, and those being installed right now.
    pub fn new_boards(&self) -> Vec<String> {
        let Ok(ports) = self.ports.lock() else {
            return Vec::new();
        };
        let Ok(fleet) = self.fleet.lock() else {
            return Vec::new();
        };
        let waiting = ports
            .iter()
            .filter(|(_, port)| port.opened.elapsed() > NEW_BOARD_AFTER)
            .filter(|(_, port)| !fleet.serves(port.link))
            .map(|(path, _)| path.clone());
        let busy = self
            .busy_ports
            .lock()
            .map(|set| set.clone())
            .unwrap_or_default();
        waiting.chain(busy).collect()
    }

    /// Which port a link of the fleet is, shortened for reading.
    pub fn port_of_link(&self, link: usize) -> Option<String> {
        let ports = self.ports.lock().ok()?;
        let found = ports.iter().find(|(_, port)| port.link == link)?;
        let name = Path::new(found.0).file_name()?;
        Some(name.to_string_lossy().to_string())
    }

    /// Frees the port, flashes and comes back to it. Runs on a thread of its own.
    fn install_on(&self, path: &str, name: &str) {
        if let Ok(mut busy) = self.busy_ports.lock() {
            busy.insert(path.to_string());
        }
        self.set_job(path, Some("installing".into()));
        // The watcher closes the port on its next look.
        thread::sleep(WATCH_EVERY * 2);
        let key = self
            .fleet
            .lock()
            .ok()
            .and_then(|fleet| fleet.public_key().ok());
        let result = match key {
            Some(key) => install_board(path, name, &self.firmware, key),
            None => Err("The control key is bad.".to_string()),
        };
        if let Ok(mut busy) = self.busy_ports.lock() {
            busy.remove(path);
        }
        if let Err(error) = &result {
            eprintln!("install: {error}");
        }
        self.set_job(path, result.err().map(|_| "install_failed".to_string()));
    }

    /// Every board heard, with what is known about how it is reached.
    pub fn board_rows(&self) -> Vec<BoardRow> {
        let seen = self
            .fleet
            .lock()
            .map(|fleet| fleet.boards())
            .unwrap_or_default();
        seen.iter()
            .map(|board| BoardRow {
                seen: board.clone(),
                on_cable: matches!(board.connection, Connection::Direct { link }
                    if self.port_of_link(link).is_some()),
                through: match board.connection {
                    Connection::Through { board: Some(id) } => seen
                        .iter()
                        .find(|known| known.id == id)
                        .map(|known| known.beacon.name.clone()),
                    _ => None,
                },
                job: self.job(&hex(&board.id)),
            })
            .collect()
    }

    /// Runs a line on a board and waits for it, which ends within seconds.
    pub fn run_line(&self, board: BoardId, line: &str) -> Result<(), LineError> {
        self.ask(board, line).map(|_| ())
    }

    /// Runs a command on a board and returns what it answered.
    pub fn ask(&self, board: BoardId, line: &str) -> Result<Option<String>, LineError> {
        if !allowed_remotely(line) {
            return Err(LineError::BadRequest);
        }
        match command(&self.fleet, board, line) {
            Ok(answer)
                if answer
                    .as_deref()
                    .is_some_and(|text| text.starts_with("error")) =>
            {
                Err(LineError::NotDone)
            }
            Ok(answer) => Ok(answer),
            Err(error) if error.starts_with("refused") => Err(LineError::Refused),
            Err(_) => Err(LineError::NoAnswer),
        }
    }

    /// Updates a board on a thread of its own. `job` shows how far it got.
    pub fn start_update(self: &Arc<Self>, board: BoardId) {
        let manager = self.clone();
        thread::spawn(move || manager.update(board));
    }

    /// Sends the firmware to a board and tells whether it came back with it.
    pub fn update(&self, board: BoardId) -> bool {
        let key = hex(&board);
        let result = self.send_firmware(board, &key);
        if let Err(error) = &result {
            eprintln!("update: {error}");
        }
        let done = result.is_ok();
        let failed = match &result {
            Err(error) if error == UNPROVEN => "unproven",
            _ => "update_failed",
        };
        self.set_job(&key, (!done).then(|| failed.to_string()));
        done
    }

    fn send_firmware(&self, board: BoardId, key: &str) -> Result<(), String> {
        self.set_job(key, Some("preparing".into()));
        let image = app_image(&self.firmware)?;
        let total = image.len() as f64;
        let shown = |next: u32| {
            self.set_job(
                key,
                Some(format!("{:.0}%", f64::from(next) * 100.0 / total)),
            )
        };
        let ledger = Ledger::open(&config_folder()?);
        let delivery = deliver(&self.fleet, &ledger, board, &image, hash_of(&image), &shown)?;
        self.set_job(key, Some("restarting".into()));
        let deadline = Instant::now() + RETURN_WAIT;
        thread::sleep(Duration::from_secs(5));
        // The beacon comes back first, and a board still catching up may miss an order.
        while Instant::now() < deadline {
            let back = self.fleet.lock().map(|fleet| {
                fleet
                    .boards()
                    .iter()
                    .any(|seen| seen.id == board && seen.silent_for < Duration::from_secs(3))
            });
            if back.unwrap_or(false) {
                let answer = command(&self.fleet, board, "status");
                if answer.is_ok_and(|line| line.is_some_and(|line| line.starts_with("status"))) {
                    return ledger.landed(&delivery);
                }
            }
            thread::sleep(Duration::from_millis(500));
        }
        Err("The board did not come back.".to_string())
    }

    /// Installs a new board on a thread of its own, if that port is waiting for it.
    pub fn start_install(self: &Arc<Self>, path: &str, name: &str) -> bool {
        let waiting = self.new_boards().iter().any(|known| known == path);
        if name.trim().is_empty() || !waiting || self.job(path).as_deref() == Some("installing") {
            return false;
        }
        let (manager, path, name) = (self.clone(), path.to_string(), name.trim().to_string());
        thread::spawn(move || manager.install_on(&path, &name));
        true
    }

    /// Runs an optional program on boards. Its lines appear in `run`.
    pub fn start_extra(
        &self,
        extra: &'static dyn Extra,
        boards: &[BoardId],
        args: Args,
    ) -> Result<(), StartError> {
        if self.is_updating() {
            return Err(StartError::Busy);
        }
        let stop = Arc::new(AtomicBool::new(false));
        {
            let Ok(mut run) = self.run.lock() else {
                return Err(StartError::Busy);
            };
            if run.running {
                return Err(StartError::Busy);
            }
            run.lines.clear();
            run.running = true;
            run.stop = Some(stop.clone());
        }
        let links: Vec<Box<dyn BoardLink>> = boards
            .iter()
            .map(|id| Box::new(RemoteBoard::new(self.fleet.clone(), *id)) as Box<dyn BoardLink>)
            .collect();
        let run = self.run.clone();
        let lines = run.clone();
        let say: Say = Arc::new(move |line| push_line(&lines, line));
        thread::spawn(move || {
            let result = extra.run_on(links, &args, say, stop);
            if let Err(error) = result {
                push_line(&run, error);
            }
            if let Ok(mut run) = run.lock() {
                run.running = false;
                run.stop = None;
            }
        });
        Ok(())
    }

    pub fn stop_extra(&self) {
        if let Ok(run) = self.run.lock() {
            if let Some(stop) = &run.stop {
                stop.store(true, Ordering::Relaxed);
            }
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn push_line(run: &Mutex<ExtraRun>, line: String) {
    if let Ok(mut run) = run.lock() {
        if run.lines.len() >= MAX_LOG {
            run.lines.pop_front();
        }
        run.lines.push_back(line);
    }
}

/// Which device file a port name leads to right now.
fn device_of(path: &str) -> Option<u64> {
    fs::metadata(path).ok().map(|found| found.ino())
}
