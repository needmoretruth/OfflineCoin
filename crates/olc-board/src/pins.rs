//! Which address each board spoke with. A signature proves who signed, not that it is the board
//! this computer knows, so a board id stays with its first address until the board says otherwise
//! over its own cable, as a wiped board does.
use olc_control::BoardId;
use olc_crypto::Address;
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

const FILE: &str = "known-boards";
/// Boards learned over the air. New keys cost nothing to make, and each one is a line in a file.
/// The cable can always add one more.
const MOST_PINS: usize = 1_024;

#[derive(Default)]
pub struct Pins {
    file: Option<PathBuf>,
    addresses: BTreeMap<BoardId, Address>,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn bytes<const N: usize>(text: &str) -> Option<[u8; N]> {
    let mut out = [0u8; N];
    if text.len() != N * 2 || !text.is_ascii() {
        return None;
    }
    for (at, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(text.get(at * 2..at * 2 + 2)?, 16).ok()?;
    }
    Some(out)
}

impl Pins {
    /// Reads what an earlier run learned from the file in `folder`.
    pub fn stored_in(folder: PathBuf) -> Self {
        let file = folder.join(FILE);
        let text = fs::read_to_string(&file).unwrap_or_default();
        let addresses = text
            .lines()
            .filter_map(|line| {
                let (board, address) = line.split_once(' ')?;
                Some((bytes(board)?, bytes(address)?))
            })
            .collect();
        Self {
            file: Some(file),
            addresses,
        }
    }

    /// True when `address` may speak for `board`. The cable is proof enough to learn a new one.
    pub fn accept(&mut self, board: BoardId, address: Address, on_cable: bool) -> bool {
        let known = self.addresses.get(&board);
        if known == Some(&address) {
            return true;
        }
        if known.is_some() && !on_cable {
            return false;
        }
        if self.addresses.len() >= MOST_PINS && !on_cable {
            return false;
        }
        self.addresses.insert(board, address);
        self.save();
        true
    }

    fn save(&self) {
        let Some(file) = &self.file else {
            return;
        };
        let lines: Vec<String> = self
            .addresses
            .iter()
            .map(|(board, address)| format!("{} {}\n", hex(board), hex(address)))
            .collect();
        // Losing this only means learning the boards again.
        let fresh = file.with_extension("new");
        if fs::write(&fresh, lines.concat()).is_ok() {
            let _ = fs::rename(&fresh, file);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOARD: BoardId = [1; 6];

    #[test]
    fn a_board_keeps_its_first_address_over_radio() {
        let mut pins = Pins::default();
        assert!(pins.accept(BOARD, [1; 20], false));
        assert!(pins.accept(BOARD, [1; 20], false));
        assert!(!pins.accept(BOARD, [2; 20], false));
    }

    #[test]
    fn the_air_cannot_fill_the_file_without_end() {
        let mut pins = Pins::default();
        for number in 0..MOST_PINS as u32 {
            let mut board = [0; 6];
            board[..4].copy_from_slice(&number.to_le_bytes());
            assert!(pins.accept(board, [1; 20], false));
        }
        assert!(!pins.accept([9; 6], [1; 20], false));
        assert!(pins.accept([9; 6], [1; 20], true));
    }

    #[test]
    fn the_cable_can_change_it() {
        let mut pins = Pins::default();
        pins.accept(BOARD, [1; 20], false);
        assert!(pins.accept(BOARD, [2; 20], true));
        assert!(!pins.accept(BOARD, [1; 20], false));
    }

    #[test]
    fn what_was_learned_survives_a_restart() {
        let folder = std::env::temp_dir().join(format!("olc-pins-{}", std::process::id()));
        fs::create_dir_all(&folder).unwrap();
        Pins::stored_in(folder.clone()).accept(BOARD, [1; 20], false);
        assert!(!Pins::stored_in(folder.clone()).accept(BOARD, [2; 20], false));
        fs::remove_dir_all(&folder).unwrap();
    }
}
