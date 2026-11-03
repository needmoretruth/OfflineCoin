//! A running node answers words sent from the `olc` program, as bitcoind answers bitcoin-cli. The
//! node keeps a door in its wallet folder: a held lock says it runs, and each word arrives as a
//! file and leaves an answer beside it. Files keep this off the network and need no port: only the
//! person who can read the wallet folder can reach it, as with bitcoind's cookie file.
use super::console::ConsoleDesk;
use super::service::Core;
use super::words::{run_words, Desk};
use olc_board::Args;
use olc_platform::platform;
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const FOLDER: &str = "terminal";
const LOCK: &str = "door.lock";
const ASK: &str = "ask";
const SAID: &str = "said";
const LOOK_EVERY: Duration = Duration::from_millis(100);
/// The slowest word a node answers, a chain still opening, and some room.
const ANSWER_WAIT: Duration = Duration::from_secs(180);

#[derive(Serialize, Deserialize)]
struct Said {
    good: bool,
    text: String,
}

/// The door of a running node: held while it runs.
pub struct Door {
    folder: PathBuf,
    _lock: File,
}

fn folder_of(wallet: &Path) -> PathBuf {
    wallet.join(FOLDER)
}

impl Door {
    /// Opens the door of the node on `wallet`. `None` when another running node holds it.
    pub fn open(wallet: &Path) -> Option<Self> {
        let folder = folder_of(wallet);
        fs::create_dir_all(&folder).ok()?;
        let lock = File::create(folder.join(LOCK)).ok()?;
        lock.try_lock().ok()?;
        // Words left from a node that ended before it answered them.
        for entry in fs::read_dir(&folder).ok()?.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|ending| ending == ASK || ending == SAID) {
                let _ = fs::remove_file(path);
            }
        }
        Some(Self { folder, _lock: lock })
    }

    /// The words waiting now, each with the name its answer goes under.
    pub fn take(&self) -> Vec<(String, Vec<String>)> {
        let Ok(entries) = fs::read_dir(&self.folder) else {
            return Vec::new();
        };
        let mut asked = Vec::new();
        for path in entries.flatten().map(|entry| entry.path()) {
            if !path.extension().is_some_and(|ending| ending == ASK) {
                continue;
            }
            let read = fs::read(&path).ok().and_then(|bytes| serde_json::from_slice::<Vec<String>>(&bytes).ok());
            let _ = fs::remove_file(&path);
            let name = path.file_stem().map(|stem| stem.to_string_lossy().into_owned());
            if let (Some(name), Some(words)) = (name, read) {
                asked.push((name, words));
            }
        }
        asked
    }

    /// Leaves the answer to the words sent as `name`.
    pub fn answer(&self, name: &str, said: &Result<String, String>) {
        leave(&self.folder, name, said);
    }

    /// Runs each word waiting on `desk` and leaves its answer.
    pub fn serve(&self, desk: &mut dyn Desk) {
        for (name, words) in self.take() {
            let said = run_words(desk, &Args::parse(words.into_iter()));
            let said = said.unwrap_or_else(|| Err("The running node has no such word.".to_string()));
            self.answer(&name, &said);
        }
    }
}

fn leave(folder: &Path, name: &str, said: &Result<String, String>) {
    let said = match said {
        Ok(text) => Said { good: true, text: text.clone() },
        Err(text) => Said { good: false, text: text.clone() },
    };
    write_whole(&folder.join(format!("{name}.{SAID}")), &serde_json::to_vec(&said).unwrap_or_default());
}

/// Written beside, then moved in, so the other side never reads half of it.
fn write_whole(path: &Path, bytes: &[u8]) {
    let part = path.with_extension("part");
    if platform().write_private(&part, bytes).is_ok() {
        let _ = fs::rename(&part, path);
    }
}

/// True when a node runs on `wallet` and keeps its door.
pub fn runs_on(wallet: &Path) -> bool {
    let Ok(lock) = File::open(folder_of(wallet).join(LOCK)) else {
        return false;
    };
    lock.try_lock().is_err()
}

/// Sends `words` to the node running on `wallet` and gives its answer. `None` when no node runs
/// there, so the words are carried out here as before.
pub fn ask(wallet: &Path, words: &[String]) -> Option<Result<String, String>> {
    if words.is_empty() || !runs_on(wallet) {
        return None;
    }
    let folder = folder_of(wallet);
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_nanos());
    let name = format!("{}-{stamp}", std::process::id());
    write_whole(&folder.join(format!("{name}.{ASK}")), &serde_json::to_vec(words).unwrap_or_default());
    let answer = folder.join(format!("{name}.{SAID}"));
    let started = Instant::now();
    while started.elapsed() < ANSWER_WAIT {
        if let Ok(bytes) = fs::read(&answer) {
            let _ = fs::remove_file(&answer);
            let said: Said = serde_json::from_slice(&bytes).ok()?;
            return Some(if said.good { Ok(said.text) } else { Err(said.text) });
        }
        thread::sleep(LOOK_EVERY);
    }
    let _ = fs::remove_file(folder.join(format!("{name}.{ASK}")));
    Some(Err("The running node did not answer in time.".to_string()))
}

/// Keeps the door of OLC Core for as long as anyone holds the core. `alive` gives the core while
/// someone does, and `None` once only the core's own thread is left.
pub(super) fn serve_core(wallet: PathBuf, alive: impl Fn() -> Option<Core> + Send + 'static) {
    thread::spawn(move || {
        let Some(door) = Door::open(&wallet) else {
            return;
        };
        while let Some(core) = alive() {
            for (name, words) in door.take() {
                let core = core.clone();
                let folder = door.folder.clone();
                // A board or a payment may take seconds; other words keep coming in meanwhile.
                thread::spawn(move || {
                    let mut desk = ConsoleDesk::new(core);
                    let said = run_words(&mut desk, &Args::parse(words.into_iter()));
                    let said = said.unwrap_or_else(|| Err("OLC Core has no such word.".to_string()));
                    leave(&folder, &name, &said);
                });
            }
            drop(core);
            thread::sleep(LOOK_EVERY);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{BoardView, Command, SwapOrder};

    struct Echo;

    impl Desk for Echo {
        fn command(&mut self, command: Command) -> Result<String, String> {
            match command {
                Command::Swap(SwapOrder::List) => Ok("the swaps listed".into()),
                _ => Ok("something else".into()),
            }
        }
        fn boards(&mut self) -> Result<Vec<BoardView>, String> {
            Ok(Vec::new())
        }
        fn password(&mut self) -> Result<String, String> {
            Err("no".into())
        }
    }

    /// With no node on the wallet, the words stay with the program that was given them.
    #[test]
    fn words_reach_the_node_running_on_the_wallet_and_nothing_else() {
        let wallet = std::env::temp_dir().join(format!("olc-door-{}", std::process::id()));
        let _ = fs::remove_dir_all(&wallet);
        fs::create_dir_all(&wallet).unwrap();
        let words: Vec<String> = ["swap", "list"].iter().map(|word| word.to_string()).collect();
        assert!(ask(&wallet, &words).is_none(), "nobody runs here");

        let door = Door::open(&wallet).unwrap();
        assert!(Door::open(&wallet).is_none(), "a second node shares the door");
        let serving = thread::spawn(move || {
            let started = Instant::now();
            while started.elapsed() < Duration::from_secs(10) {
                door.serve(&mut Echo);
                thread::sleep(LOOK_EVERY);
            }
        });
        let said = ask(&wallet, &words).unwrap().unwrap();
        assert_eq!(said, "the swaps listed");
        let refused = ask(&wallet, &["nothing".to_string()]).unwrap();
        assert!(refused.is_err());
        serving.join().unwrap();
        assert!(!runs_on(&wallet), "the door closes with the node");
        fs::remove_dir_all(&wallet).unwrap();
    }

    /// OLC Core keeps the door of its wallet, so the swap words of `olc` reach the core that runs
    /// the swaps.
    #[test]
    fn swap_words_from_the_terminal_reach_a_running_core() {
        use crate::core::CoreLaunch;
        use crate::settings::{Paths, Settings};
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.scratch/tests/door-core");
        let _ = fs::remove_dir_all(&root);
        let paths = Paths { root: root.join("core") };
        let wallet = paths.wallet();
        let settings = Settings { port: 7690, setup_done: false, listen_lan: false, scan_lan: false, ..Settings::default() };
        let launch = CoreLaunch { paths, fleet: None, usb: false, node_program: Some("/bin/true".into()) };
        let core = Core::start(launch, settings).unwrap();
        let started = Instant::now();
        while !runs_on(&wallet) {
            assert!(started.elapsed() < Duration::from_secs(10), "the core kept no door");
            thread::sleep(LOOK_EVERY);
        }
        let words: Vec<String> = ["swap", "list"].iter().map(|word| word.to_string()).collect();
        let said = ask(&wallet, &words).unwrap();
        assert!(said.is_ok(), "{said:?}");
        core.finish();
    }
}
