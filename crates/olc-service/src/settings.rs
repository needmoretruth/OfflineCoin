//! What a person chose, saved as one small file. A device choice only fills these numbers: each
//! stays editable and nothing is blocked by it.
use crate::local::NodeSpec;
use olc_control::BoardId;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// How much the screens say. Basic is the app. Expert shows more where the words differ.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Mode {
    #[default]
    Basic,
    Expert,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Lang {
    #[default]
    En,
    Ko,
}

impl Lang {
    pub fn other(self) -> Self {
        match self {
            Self::En => Self::Ko,
            Self::Ko => Self::En,
        }
    }

    /// The app's language for a system language code: Korean for `ko`, English for any other.
    pub fn for_system(code: Option<&str>) -> Self {
        if code == Some("ko") {
            Self::Ko
        } else {
            Self::En
        }
    }

    /// The language this computer's system speaks.
    pub fn of_system() -> Self {
        Self::for_system(olc_platform::platform().language().as_deref())
    }
}

const OLD_BOARD_CAP: u64 = 3_000;
pub const NODE_PORT: u16 = 7000;
/// Ports tried after the chosen one when it is taken.
pub const PORT_TRIES: u16 = 10;
/// A computer's node, relaxed.
pub const RELAXED_CAP: u64 = 20_000;
pub const BOARD_CAP: u64 = olc_control::defaults::BOARD_CAP as u64;
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
    ThisComputer,
    SeveralNodes,
    /// A first screen once offered a phone as well, and old settings still say so.
    #[serde(alias = "ComputerAndPhone")]
    TwoComputers,
    RaspberryPi,
    Esp32,
}

impl Device {
    pub const ALL: [Self; 5] = [
        Self::ThisComputer,
        Self::SeveralNodes,
        Self::TwoComputers,
        Self::RaspberryPi,
        Self::Esp32,
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
            Self::ThisComputer => Defaults { listen_lan: false, ..standard },
            Self::SeveralNodes => Defaults {
                local_nodes: 3,
                listen_lan: false,
                ..standard
            },
            Self::Esp32 | Self::RaspberryPi | Self::TwoComputers => {
                standard
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub language: Lang,
    /// The person picked `language` in Settings. Until then it follows the system's language.
    pub language_chosen: bool,
    #[serde(default)]
    pub mode: Mode,
    pub theme: Theme,
    pub reduce_motion: bool,
    /// How large the whole window is drawn, in percent.
    pub text_percent: u16,
    pub setup_done: bool,
    pub device: Device,
    /// Hashes per second on this computer. 0 is no cap.
    pub hash_cap: u64,
    /// The cap offered first for a board.
    pub board_cap: u64,
    /// The person typed `board_cap`, so no later default replaces it.
    #[serde(default)]
    pub board_cap_chosen: bool,
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
            language_chosen: false,
            mode: Mode::Basic,
            theme: Theme::System,
            reduce_motion: false,
            text_percent: 100,
            setup_done: false,
            device: Device::default(),
            hash_cap: defaults.hash_cap,
            board_cap: defaults.board_cap,
            board_cap_chosen: false,
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
        self.auto_update = from.auto_update;
        self.auto_boards.clone_from(&from.auto_boards);
        self.language = from.language;
        self.language_chosen = from.language_chosen;
        self.mode = from.mode;
        self.theme = from.theme;
        self.reduce_motion = from.reduce_motion;
        self.text_percent = from.text_percent;
        self.device = from.device;
        self.hash_cap = from.hash_cap;
        self.board_cap = from.board_cap;
        self.board_cap_chosen = from.board_cap_chosen;
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
        self.board_cap_chosen = false;
        self.listen_lan = defaults.listen_lan;
    }

    /// Joined nodes and the addresses a person added, wherever they are, as `addnode` does.
    /// This node's own address is left out: dialling itself would only hold a line open.
    pub fn to_dial(&self, own_port: Option<u16>) -> Vec<String> {
        let own = |address: &str| {
            let Some((host, port)) = address.rsplit_once(':') else {
                return false;
            };
            let here = matches!(host, "127.0.0.1" | "localhost" | "[::1]" | "::1");
            here && port.parse::<u16>().ok() == own_port
        };
        let mut dial: Vec<String> = Vec::new();
        for address in self.peers.iter().chain(&self.added) {
            if !own(address) && !dial.contains(address) {
                dial.push(address.clone());
            }
        }
        dial
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
        Self::load_for(root, Lang::of_system())
    }

    /// What `load` gives on a system that speaks `system`.
    pub fn load_for(root: &Path, system: Lang) -> Self {
        let text = fs::read_to_string(root.join(FILE)).ok();
        let saved = text.as_deref().and_then(|text| {
            let settings = serde_json::from_str::<Self>(text).ok()?;
            Some(settings.with_old_pick(text.contains("\"language_chosen\"")))
        });
        saved.unwrap_or_default().moved_to_shared_cap().following(system)
    }

    /// A file from before the pick was kept held English as the default, so only Korean in it
    /// was a person's pick.
    fn with_old_pick(mut self, knows_picks: bool) -> Self {
        if !knows_picks && self.language == Lang::Ko {
            self.language_chosen = true;
        }
        self
    }

    /// The system's language, unless the person picked one.
    fn following(mut self, system: Lang) -> Self {
        if !self.language_chosen {
            self.language = system;
        }
        self
    }

    /// `OLD_BOARD_CAP` was once the default, so a saved one was no choice.
    fn moved_to_shared_cap(mut self) -> Self {
        if self.board_cap == OLD_BOARD_CAP && !self.board_cap_chosen {
            self.board_cap = BOARD_CAP;
        }
        self
    }

    pub fn save(&self, root: &Path) -> Result<(), String> {
        let text = serde_json::to_string_pretty(self).map_err(|error| error.to_string())?;
        let spare = root.join(format!("{FILE}.new"));
        fs::write(&spare, text).map_err(|error| error.to_string())?;
        fs::rename(spare, root.join(FILE)).map_err(|error| error.to_string())
    }
}

/// Where this person's data lives, by the custom of the system.
pub fn standard_folder() -> Result<PathBuf, String> {
    let folder = olc_platform::platform().data_folder();
    folder.ok_or_else(|| "This system gave no folder for data.".to_string())
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
    fn an_old_default_board_cap_moves_and_a_chosen_one_stays() {
        let old = Settings { board_cap: OLD_BOARD_CAP, ..Settings::default() };
        assert_eq!(old.moved_to_shared_cap().board_cap, BOARD_CAP);
        let chosen = Settings {
            board_cap: OLD_BOARD_CAP,
            board_cap_chosen: true,
            ..Settings::default()
        };
        assert_eq!(chosen.moved_to_shared_cap().board_cap, OLD_BOARD_CAP);
    }

    #[test]
    fn the_update_switch_starts_off_and_is_stored() {
        assert!(!Settings::default().auto_update);
        let mut stored = Settings::default();
        let chosen = Settings {
            auto_update: true,
            auto_boards: vec![[7; 6]],
            ..Settings::default()
        };
        stored.take_choices(&chosen);
        assert!(stored.auto_update);
        assert_eq!(stored.auto_boards, vec![[7; 6]]);
    }

    #[test]
    fn a_device_choice_only_fills_numbers() {
        let mut settings = Settings::default();
        assert_eq!(settings.device, Device::ThisComputer);
        assert_eq!(settings.device.defaults().local_nodes, 0);
        settings.apply_device(Device::SeveralNodes);
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

    fn folder_with(name: &str, file: Option<&str>) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("olc-lang-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).unwrap();
        if let Some(text) = file {
            fs::write(folder.join(FILE), text).unwrap();
        }
        folder
    }

    #[test]
    fn a_first_start_speaks_the_system_language() {
        assert_eq!(Lang::for_system(Some("ko")), Lang::Ko);
        assert_eq!(Lang::for_system(Some("en")), Lang::En);
        assert_eq!(Lang::for_system(Some("de")), Lang::En);
        assert_eq!(Lang::for_system(None), Lang::En);
        let folder = folder_with("first", None);
        assert_eq!(Settings::load_for(&folder, Lang::Ko).language, Lang::Ko);
        assert_eq!(Settings::load_for(&folder, Lang::En).language, Lang::En);
        fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn a_picked_language_stays_on_any_system() {
        let folder = folder_with("picked", None);
        let mut settings = Settings::load_for(&folder, Lang::Ko);
        settings.language = Lang::En;
        settings.language_chosen = true;
        settings.save(&folder).unwrap();
        assert_eq!(Settings::load_for(&folder, Lang::Ko).language, Lang::En);
        let unpicked = Settings { language: Lang::Ko, ..Settings::default() };
        unpicked.save(&folder).unwrap();
        assert_eq!(Settings::load_for(&folder, Lang::En).language, Lang::En);
        fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn an_old_file_follows_the_system_unless_it_held_a_pick() {
        let default = folder_with("old-default", Some(r#"{"language":"En","setup_done":true}"#));
        let settings = Settings::load_for(&default, Lang::Ko);
        assert_eq!(settings.language, Lang::Ko);
        assert!(settings.setup_done);
        let picked = folder_with("old-picked", Some(r#"{"language":"Ko","setup_done":true}"#));
        assert_eq!(Settings::load_for(&picked, Lang::En).language, Lang::Ko);
        fs::remove_dir_all(&default).unwrap();
        fs::remove_dir_all(&picked).unwrap();
    }
}
