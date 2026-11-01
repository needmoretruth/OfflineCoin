//! One running copy per person. A second start asks the first to come forward and ends, since two
//! nodes would fight over the cables and the wallet. A file socket keeps this off the network.
use std::fs::{self, File};
use std::io::Write;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use std::thread;

const LOCK: &str = "core.lock";
const SOCKET: &str = "core.socket";

pub struct Instance {
    _lock: File,
    socket: PathBuf,
    /// A message per later start.
    pub raised: Receiver<()>,
}

impl Drop for Instance {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.socket);
    }
}

/// `None` when a copy already runs. That copy has been told to come forward.
pub fn claim(root: &Path) -> Result<Option<Instance>, String> {
    fs::create_dir_all(root).map_err(|error| error.to_string())?;
    let lock = File::create(root.join(LOCK)).map_err(|error| error.to_string())?;
    let socket = root.join(SOCKET);
    if lock.try_lock().is_err() {
        if let Ok(mut stream) = UnixStream::connect(&socket) {
            let _ = stream.write_all(b"show");
        }
        return Ok(None);
    }
    let _ = fs::remove_file(&socket);
    let listener = UnixListener::bind(&socket).map_err(|error| error.to_string())?;
    let (sender, raised) = channel();
    thread::spawn(move || {
        for stream in listener.incoming() {
            if stream.is_ok() && sender.send(()).is_err() {
                return;
            }
        }
    });
    Ok(Some(Instance {
        _lock: lock,
        socket,
        raised,
    }))
}
