//! What keeps a build from stranding a board nobody can reach by hand.
//!
//! The bootloader on these boards keeps no earlier build to fall back on. A board with a cable
//! can always be flashed again, so it may try anything. A board on power alone takes a build
//! only once that build has shown, on a board with a cable, that it answers by radio and takes
//! an update by radio. That proof is two deliveries of the same image: the first puts it on the
//! board, the second reaches the running build by radio and it comes back from that.
use crate::fleet::Connection;
use crate::ports::board_ports;
use crate::remote::{send_image, SharedFleet};
use olc_control::BoardId;
use olc_crypto::Hash;
use std::fmt::Write as _;
use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

const LEDGER_FILE: &str = "proven-builds";
pub const UNPROVEN: &str = "This build has not proved itself on a board with a cable yet.";

/// Which builds have proved themselves, kept in the configuration folder.
pub struct Ledger {
    file: PathBuf,
    cables: fn() -> Vec<String>,
}

/// A build this board may take. Only [`Ledger`] makes one, so nothing sends without asking.
pub struct Cleared {
    pub(crate) board: BoardId,
    pub(crate) hash: Hash,
}

/// An image that reached a board. Tell the ledger once the board is back and answers.
pub struct Delivery {
    pub took: Duration,
    cleared: Cleared,
    by_radio: bool,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut text, byte| {
        let _ = write!(text, "{byte:02x}");
        text
    })
}

/// A board gives its MAC address as its USB serial number.
fn cabled_boards() -> Vec<String> {
    board_ports().into_iter().map(|port| port.serial).collect()
}

impl Ledger {
    pub fn open(folder: &Path) -> Self {
        Self::at(folder.join(LEDGER_FILE), cabled_boards)
    }

    fn at(file: PathBuf, cables: fn() -> Vec<String>) -> Self {
        Self { file, cables }
    }

    /// Whether this computer has a cable to the board, used or not.
    pub fn has_cable(&self, board: BoardId) -> bool {
        let mac: Vec<String> = board.iter().map(|byte| format!("{byte:02X}")).collect();
        let mac = mac.join(":");
        (self.cables)().iter().any(|serial| serial.eq_ignore_ascii_case(&mac))
    }

    fn lines(&self) -> Vec<String> {
        let text = fs::read_to_string(&self.file).unwrap_or_default();
        text.lines().map(str::to_string).collect()
    }

    fn add(&self, line: &str) -> Result<(), String> {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.file)
            .map_err(|error| error.to_string())?;
        writeln!(file, "{line}").map_err(|error| error.to_string())
    }

    pub fn is_proven(&self, hash: &Hash) -> bool {
        let wanted = format!("proven {}", hex(hash));
        self.lines().contains(&wanted)
    }

    pub fn clear(&self, board: BoardId, hash: Hash) -> Result<Cleared, String> {
        if self.has_cable(board) || self.is_proven(&hash) {
            Ok(Cleared { board, hash })
        } else {
            Err(UNPROVEN.to_string())
        }
    }

    /// Notes a delivery to a board with a cable that came back. Reaching by radio a board that
    /// already ran this same image is the proof.
    pub fn landed(&self, delivery: &Delivery) -> Result<(), String> {
        let Cleared { board, hash } = &delivery.cleared;
        if !self.has_cable(*board) {
            return Ok(());
        }
        let ran = format!("ran {} ", hex(board));
        let before = self
            .lines()
            .into_iter()
            .rfind(|line| line.starts_with(&ran));
        let now = format!("{ran}{}", hex(hash));
        if delivery.by_radio && before.as_deref() == Some(now.as_str()) {
            self.add(&format!("proven {}", hex(hash)))?;
        }
        self.add(&now)
    }
}

/// Sends an image the ledger allows. The board restarts on its own once it has checked it.
pub fn deliver(
    fleet: &SharedFleet,
    ledger: &Ledger,
    board: BoardId,
    image: &[u8],
    hash: Hash,
    on_progress: &dyn Fn(u32),
) -> Result<Delivery, String> {
    let cleared = ledger.clear(board, hash)?;
    let by_radio = fleet
        .lock()
        .map_err(|_| "The board list stopped working.")?
        .boards()
        .iter()
        .any(|seen| seen.id == board && matches!(seen.connection, Connection::Through { .. }));
    let took = send_image(fleet, &cleared, image, on_progress)?;
    Ok(Delivery {
        took,
        cleared,
        by_radio,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control_key::ControlKey;
    use crate::fleet::Fleet;
    use olc_node::Runner;
    use std::sync::{Arc, Mutex};

    const BOARD: BoardId = [0xAC, 0x27, 0x6E, 0xAA, 0xEF, 0x90];

    fn ledger(name: &str, cable: bool) -> Ledger {
        let folder = std::env::temp_dir().join(format!("olc-clear-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).unwrap();
        let cables = if cable { one_cable } else { no_cable };
        Ledger::at(folder.join(LEDGER_FILE), cables)
    }

    fn one_cable() -> Vec<String> {
        vec!["AC:27:6E:AA:EF:90".into()]
    }

    fn no_cable() -> Vec<String> {
        Vec::new()
    }

    fn delivery(by_radio: bool, hash: Hash) -> Delivery {
        Delivery {
            took: Duration::ZERO,
            cleared: Cleared { board: BOARD, hash },
            by_radio,
        }
    }

    #[test]
    fn a_board_with_a_cable_may_try_anything() {
        let ledger = ledger("cable", true);
        assert!(ledger.clear(BOARD, [1; 32]).is_ok());
    }

    #[test]
    fn a_board_on_power_alone_refuses_an_unproven_build() {
        let ledger = ledger("alone", false);
        assert_eq!(
            ledger.clear(BOARD, [1; 32]).err().as_deref(),
            Some(UNPROVEN)
        );
    }

    #[test]
    fn nothing_leaves_for_a_board_on_power_alone_until_the_build_is_proved() {
        let folder = std::env::temp_dir().join(format!("olc-leave-{}", std::process::id()));
        fs::create_dir_all(&folder).unwrap();
        let key = ControlKey::load_or_create(&folder).unwrap();
        let fleet = Fleet::new(key, Runner::link_of).unwrap();
        let fleet: SharedFleet = Arc::new(Mutex::new(fleet));
        let ledger = ledger("leave", false);
        let image = [7u8; 64];
        let sent = deliver(&fleet, &ledger, BOARD, &image, [1; 32], &|_| {});
        assert_eq!(sent.err().as_deref(), Some(UNPROVEN));
        assert!(fleet.lock().unwrap().take_outgoing().is_none());
        let _ = fs::remove_dir_all(&folder);
    }

    #[test]
    fn the_same_image_reaching_a_running_board_by_radio_is_the_proof() {
        let ledger = ledger("proof", true);
        let hash = [2; 32];
        ledger.landed(&delivery(false, hash)).unwrap();
        assert!(!ledger.is_proven(&hash), "one delivery proves nothing");
        ledger.landed(&delivery(true, hash)).unwrap();
        assert!(ledger.is_proven(&hash));
    }

    #[test]
    fn a_second_delivery_by_cable_proves_nothing() {
        let ledger = ledger("usb", true);
        let hash = [3; 32];
        ledger.landed(&delivery(false, hash)).unwrap();
        ledger.landed(&delivery(false, hash)).unwrap();
        assert!(!ledger.is_proven(&hash));
    }

    #[test]
    fn a_different_image_in_between_starts_over() {
        let ledger = ledger("between", true);
        ledger.landed(&delivery(false, [4; 32])).unwrap();
        ledger.landed(&delivery(true, [5; 32])).unwrap();
        ledger.landed(&delivery(true, [4; 32])).unwrap();
        assert!(!ledger.is_proven(&[4; 32]));
    }

    #[test]
    fn a_board_on_power_alone_proves_nothing() {
        let ledger = ledger("nothing", false);
        let hash = [6; 32];
        ledger.landed(&delivery(true, hash)).unwrap();
        ledger.landed(&delivery(true, hash)).unwrap();
        assert!(!ledger.is_proven(&hash));
    }
}
