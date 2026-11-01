//! One running copy per person. A second start asks the first to come forward and ends, since two
//! nodes would fight over the cables and the wallet. A file carries the request, which keeps this
//! off the network on every system.
use std::fs::{self, File};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

const LOCK: &str = "core.lock";
const RAISE: &str = "core.raise";
const LOOK_EVERY: Duration = Duration::from_millis(400);

pub struct Instance {
    _lock: File,
    stop: Arc<AtomicBool>,
    /// A message per later start.
    pub raised: Receiver<()>,
}

impl Drop for Instance {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// `None` when a copy already runs. That copy has been told to come forward.
pub fn claim(root: &Path) -> Result<Option<Instance>, String> {
    fs::create_dir_all(root).map_err(|error| error.to_string())?;
    let lock = File::create(root.join(LOCK)).map_err(|error| error.to_string())?;
    let request = root.join(RAISE);
    if lock.try_lock().is_err() {
        let _ = fs::write(&request, []);
        return Ok(None);
    }
    let _ = fs::remove_file(&request);
    let (sender, raised) = channel();
    let stop = Arc::new(AtomicBool::new(false));
    let stopped = stop.clone();
    thread::spawn(move || {
        while !stopped.load(Ordering::Relaxed) {
            if fs::remove_file(&request).is_ok() && sender.send(()).is_err() {
                return;
            }
            thread::sleep(LOOK_EVERY);
        }
    });
    Ok(Some(Instance {
        _lock: lock,
        stop,
        raised,
    }))
}
