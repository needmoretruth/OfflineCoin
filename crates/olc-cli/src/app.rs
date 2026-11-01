//! What the app adds to a node: a [`Manager`] for the boards, and what the page needs from it.
use olc_board::carried::firmware_path;
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
