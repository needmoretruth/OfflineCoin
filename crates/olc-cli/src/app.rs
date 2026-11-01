//! What the app adds to a node: the boards on its cables, the fleet they reach, and the jobs a
//! person starts from the page.
use crate::node::Shared;
use olc_board::control_key::{config_folder, ControlKey};
use olc_board::install::install as install_board;
use olc_board::{Args, Fleet, SharedFleet};
use olc_control::Forward;
use olc_link_serial::SerialLink;
use olc_node::{Message, Outgoing, Runner};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const FIRMWARE_NAME: &str = "olc-firmware";
const DEFAULT_FIRMWARE: &str = "../../firmware/target/xtensa-esp32s3-none-elf/release/olc-firmware";
const PORT_FOLDER: &str = "/dev/serial/by-id";
const PORT_MARK: &str = "Espressif";
const WATCH_EVERY: Duration = Duration::from_secs(2);
/// A port stays quiet this long before the page offers to install on it.
const NEW_BOARD_AFTER: Duration = Duration::from_secs(14);
const MAX_LOG: usize = 400;

pub struct Port {
    link: usize,
    opened: Instant,
    closer: Arc<AtomicBool>,
}

/// A run of an optional program, as the page sees it.
#[derive(Default)]
pub struct ExtraRun {
    pub lines: VecDeque<String>,
    pub running: bool,
    pub stop: Option<Arc<AtomicBool>>,
}

pub struct App {
    pub fleet: SharedFleet,
    pub firmware: PathBuf,
    pub web_port: AtomicU16,
    page_seen: Mutex<Option<Instant>>,
    /// Given on the command line. Without any, the app finds boards by itself.
    fixed_ports: Vec<String>,
    ports: Mutex<BTreeMap<String, Port>>,
    busy_ports: Mutex<BTreeSet<String>>,
    /// What is happening to a board or a port, for the page.
    pub jobs: Mutex<BTreeMap<String, String>>,
    pub run: Arc<Mutex<ExtraRun>>,
}

/// The file next to the program when it was installed, else the one this source tree builds, so
/// the app works from any folder.
fn firmware_path(args: &Args) -> PathBuf {
    if let Some(chosen) = args.value("firmware") {
        return PathBuf::from(chosen);
    }
    let beside_program = std::env::current_exe()
        .ok()
        .and_then(|program| Some(program.parent()?.join(FIRMWARE_NAME)));
    let built = Path::new(env!("CARGO_MANIFEST_DIR")).join(DEFAULT_FIRMWARE);
    beside_program.filter(|file| file.exists()).unwrap_or(built)
}

fn control_key() -> Result<ControlKey, String> {
    ControlKey::load_or_create(&config_folder()?)
}

/// `olc install`: the same steps as the page, for a port given by hand.
pub fn install(args: &Args) -> Result<(), String> {
    let port = args.value("port").ok_or("--port needs a port")?;
    let name = args.value("name").ok_or("--name needs a name")?;
    install_board(port, name, &firmware_path(args), control_key()?.public()?)
}

impl App {
    pub fn new(args: &Args) -> Result<Arc<Self>, String> {
        let fleet = Fleet::new(control_key()?, Runner::link_of)?;
        Ok(Arc::new(Self {
            fleet: Arc::new(Mutex::new(fleet)),
            firmware: firmware_path(args),
            web_port: AtomicU16::new(0),
            page_seen: Mutex::new(None),
            fixed_ports: args.values("serial").to_vec(),
            ports: Mutex::new(BTreeMap::new()),
            busy_ports: Mutex::new(BTreeSet::new()),
            jobs: Mutex::new(BTreeMap::new()),
            run: Arc::new(Mutex::new(ExtraRun::default())),
        }))
    }

    /// Moves control packets between the links and the fleet.
    pub fn page_was_here(&self) {
        if let Ok(mut seen) = self.page_seen.lock() {
            *seen = Some(Instant::now());
        }
    }

    /// True once a page has been open and has stayed away this long.
    pub fn page_gone_for(&self, wait: Duration) -> bool {
        let seen = self.page_seen.lock().ok().and_then(|seen| *seen);
        seen.is_some_and(|seen| seen.elapsed() >= wait)
    }

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

    /// Opens what appeared and lets go of what left.
    fn watch(&self, shared: &Shared) {
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
            let keep = present.contains(path) && !busy.contains(path);
            if !keep {
                port.closer.store(true, Ordering::Relaxed);
            }
            keep
        });
        for path in present {
            if ports.contains_key(&path) || busy.contains(&path) {
                continue;
            }
            let Ok(link) = SerialLink::open(&path, |_| {}) else {
                continue;
            };
            let closer = link.closer();
            if let Ok(mut runner) = shared.runner.lock() {
                let link = runner.add_link(Box::new(link));
                let opened = Instant::now();
                ports.insert(
                    path,
                    Port {
                        link,
                        opened,
                        closer,
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
    pub fn install_on(&self, path: &str, name: &str) {
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
}

pub fn push_line(run: &Mutex<ExtraRun>, line: String) {
    if let Ok(mut run) = run.lock() {
        if run.lines.len() >= MAX_LOG {
            run.lines.pop_front();
        }
        run.lines.push_back(line);
    }
}

pub fn watch_ports(shared: &Arc<Shared>) {
    if shared.app.is_none() {
        return;
    }
    let shared = shared.clone();
    thread::spawn(move || loop {
        if let Some(app) = &shared.app {
            app.watch(&shared);
        }
        thread::sleep(WATCH_EVERY);
    });
}
