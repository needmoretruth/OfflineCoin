//! What a person chose, saved as one small file. A device choice only fills these numbers: each
//! stays editable and nothing is blocked by it.
use crate::i18n::Lang;
use crate::local::NodeSpec;
use olc_control::BoardId;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

pub const NODE_PORT: u16 = 7000;
/// Ports tried after the chosen one when it is taken.
pub const PORT_TRIES: u16 = 10;
/// A computer's node, relaxed.
pub const RELAXED_CAP: u64 = 20_000;
pub const BOARD_CAP: u64 = 3_000;
pub const NEW_COIN_DIFFICULTY: u64 = 30_000;
const FILE: &str = "core.json";
const MAX_BOOK: usize = 64;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

/// What a person says they are using on the first screen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Device {
    #[default]
    Esp32,
    RaspberryPi,
    ThisComputer,
    ComputerAndPhone,
    TwoComputers,
}

impl Device {
    pub const ALL: [Self; 5] = [
        Self::Esp32,
        Self::RaspberryPi,
        Self::ThisComputer,
        Self::ComputerAndPhone,
        Self::TwoComputers,
    ];
}

/// The numbers a device choice fills in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Defaults {
    pub hash_cap: u64,
    pub board_cap: u64,
    /// Node processes to create beside the app's own node.
    pub local_nodes: u32,
    /// Whether other computers may connect to this one.
    pub listen_lan: bool,
}

impl Device {
    pub fn defaults(self) -> Defaults {
        let standard = Defaults {
            hash_cap: RELAXED_CAP,
            board_cap: BOARD_CAP,
            local_nodes: 0,
            listen_lan: true,
        };
        match self {
            Self::ThisComputer => Defaults {
                local_nodes: 3,
                listen_lan: false,
                ..standard
            },
            Self::Esp32 | Self::RaspberryPi | Self::ComputerAndPhone | Self::TwoComputers => {
                standard
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub language: Lang,
    pub theme: Theme,
    pub reduce_motion: bool,
    pub setup_done: bool,
    pub device: Device,
    /// Hashes per second on this computer. 0 is no cap.
    pub hash_cap: u64,
    /// The cap offered first for a board.
    pub board_cap: u64,
    pub mine_on_start: bool,
    /// Node processes on this computer.
    pub nodes: Vec<NodeSpec>,
    pub port: u16,
    pub listen_lan: bool,
    pub scan_lan: bool,
    pub broadcast: bool,
    /// Extra ports to try on every address.
    pub scan_ports: Vec<u16>,
    /// The folder name of the coin this app runs.
    pub coin: Option<String>,
    /// Whether picked boards follow the build this program carries. Off until someone says so.
    pub auto_update: bool,
    /// The boards an automatic update may touch.
    pub auto_boards: Vec<BoardId>,
    /// The nodes this app joined, as `host:port`.
    pub peers: Vec<String>,
    /// Addresses a person added.
    pub added: Vec<String>,
    /// Addresses learned from other nodes, so they survive a restart.
    pub book: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        let defaults = Device::default().defaults();
        Self {
            language: Lang::En,
            theme: Theme::System,
            reduce_motion: false,
            setup_done: false,
            device: Device::default(),
            hash_cap: defaults.hash_cap,
            board_cap: defaults.board_cap,
            mine_on_start: false,
            auto_update: false,
            auto_boards: Vec::new(),
            nodes: Vec::new(),
            port: NODE_PORT,
            listen_lan: defaults.listen_lan,
            scan_lan: true,
            broadcast: true,
            scan_ports: Vec::new(),
            coin: None,
            peers: Vec::new(),
            added: Vec::new(),
            book: Vec::new(),
        }
    }
}

impl Settings {
    /// Takes what the Settings page edits and keeps what the core itself keeps here.
    pub fn take_choices(&mut self, from: &Self) {
        self.language = from.language;
        self.theme = from.theme;
        self.reduce_motion = from.reduce_motion;
        self.device = from.device;
        self.hash_cap = from.hash_cap;
        self.board_cap = from.board_cap;
        self.mine_on_start = from.mine_on_start;
        self.port = from.port;
        self.listen_lan = from.listen_lan;
        self.scan_lan = from.scan_lan;
        self.broadcast = from.broadcast;
        self.scan_ports.clone_from(&from.scan_ports);
    }

    pub fn apply_device(&mut self, device: Device) {
        let defaults = device.defaults();
        self.device = device;
        self.hash_cap = defaults.hash_cap;
        self.board_cap = defaults.board_cap;
        self.listen_lan = defaults.listen_lan;
    }

    pub fn remember(&mut self, address: &str) {
        if !self.book.iter().any(|known| known == address) {
            self.book.push(address.to_string());
            if self.book.len() > MAX_BOOK {
                self.book.remove(0);
            }
        }
    }

    pub fn load(root: &Path) -> Self {
        fs::read_to_string(root.join(FILE))
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, root: &Path) -> Result<(), String> {
        let text = serde_json::to_string_pretty(self).map_err(|error| error.to_string())?;
        let spare = root.join(format!("{FILE}.new"));
        fs::write(&spare, text).map_err(|error| error.to_string())?;
        fs::rename(spare, root.join(FILE)).map_err(|error| error.to_string())
    }
}

/// Where this app keeps its files.
#[derive(Clone, Debug)]
pub struct Paths {
    pub root: PathBuf,
}

impl Paths {
    /// The wallet stays where `olc app` kept it, so nobody loses coins by changing program.
    pub fn wallet(&self) -> PathBuf {
        self.root.join("node")
    }

    pub fn coin(&self, name: &str) -> PathBuf {
        self.root.join("coins").join(name)
    }

    /// A node process's own folder, for its chain and its wallet.
    pub fn local_node(&self, id: u32) -> PathBuf {
        self.root.join("local").join(id.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_device_choice_only_fills_numbers() {
        let mut settings = Settings::default();
        settings.apply_device(Device::ThisComputer);
        assert_eq!(settings.device.defaults().local_nodes, 3);
        assert!(!settings.listen_lan);
        settings.hash_cap = 5;
        assert_eq!(settings.hash_cap, 5);
        assert_eq!(Device::Esp32.defaults().hash_cap, RELAXED_CAP);
        assert_eq!(Device::Esp32.defaults().board_cap, BOARD_CAP);
    }

    #[test]
    fn an_older_file_keeps_its_choices_and_gets_defaults() {
        let saved: Settings = serde_json::from_str(r#"{"language":"Ko","port":7100}"#).unwrap();
        assert_eq!(saved.language, Lang::Ko);
        assert_eq!(saved.port, 7100);
        assert!(saved.scan_lan);
    }
}
