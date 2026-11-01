//! What the app adds to a node: a [`Manager`] for the boards, and what the page needs from it.
use olc_board::carried::firmware_path;
use olc_board::control_key::{config_folder, ControlKey};
use olc_board::install::install as install_board;
use olc_board::{Args, Manager};
use std::ops::Deref;
use std::sync::atomic::AtomicU16;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub struct App {
    manager: Arc<Manager>,
    pub web_port: AtomicU16,
    page_seen: Mutex<Option<Instant>>,
}

impl Deref for App {
    type Target = Arc<Manager>;

    fn deref(&self) -> &Self::Target {
        &self.manager
    }
}

/// `olc install`: the same steps as the page, for a port given by hand.
pub fn install(args: &Args) -> Result<(), String> {
    let port = args.value("port").ok_or("--port needs a port")?;
    let name = args.value("name").ok_or("--name needs a name")?;
    let key = ControlKey::load_or_create(&config_folder()?)?.public()?;
    install_board(port, name, &firmware_path(args.value("firmware")), key)
}

impl App {
    pub fn new(args: &Args) -> Result<Arc<Self>, String> {
        let firmware = firmware_path(args.value("firmware"));
        let manager = Manager::new(firmware, args.values("serial").to_vec())?;
        Ok(Arc::new(Self {
            manager,
            web_port: AtomicU16::new(0),
            page_seen: Mutex::new(None),
        }))
    }

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
}
