//! What a person chose, saved as one small file. A value a person set stays as they set it: a
//! device choice only says how many miners to make, and a save takes only what changed.
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

/// The cap every computer once got from its device choice, so a saved one was no choice.
const OLD_COMPUTER_CAP: u64 = 20_000;
/// The defaults this file is written under. 2: no cap, listening, threads.
const FORMAT: u32 = 2;
pub const NODE_PORT: u16 = 7000;
/// Ports tried after the chosen one when it is taken.
pub const PORT_TRIES: u16 = 10;
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

/// What a device choice adds. It sets no cap and no switch: those are a person's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Defaults {
    /// Node processes to create beside the app's own node.
    pub local_nodes: u32,
}

impl Device {
    pub fn defaults(self) -> Defaults {
        match self {
            Self::SeveralNodes => Defaults { local_nodes: 3 },
            Self::ThisComputer | Self::Esp32 | Self::RaspberryPi | Self::TwoComputers => {
                Defaults { local_nodes: 0 }
            }
        }
    }
}

/// The cores a computer mines on unless a person says otherwise: all but one, so the computer
/// stays usable while it mines, as cpuminer's `--threads` counts them.
pub fn standard_threads() -> usize {
    cores().saturating_sub(1).max(1)
}

/// The cores this computer has.
pub fn cores() -> usize {
    std::thread::available_parallelism().map_or(1, |cores| cores.get())
}

/// Mining speeds as parts of what this computer does at full speed, so a step means the same on
/// any computer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Speed {
    Quarter,
    Half,
    Full,
}

impl Speed {
    pub const ALL: [Self; 3] = [Self::Quarter, Self::Half, Self::Full];

    /// The hash cap for this step on a computer whose full rate is `full`. Full is no cap.
    pub fn cap(self, full: u64) -> u64 {
        match self {
            Self::Quarter => (full / 4).max(1),
            Self::Half => (full / 2).max(1),
            Self::Full => 0,
        }
    }

    /// The step a cap stands for, if it is one.
    pub fn of(cap: u64, full: u64) -> Option<Self> {
        Self::ALL.into_iter().find(|speed| speed.cap(full) == cap)
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
    /// Hashes per second on this computer, for all its mining threads together. 0 is no cap.
    pub hash_cap: u64,
    /// Threads that mine. 0 is all cores but one.
    pub mining_threads: u32,
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
    /// The defaults the file was written under. A file without it is older than all of them.
    #[serde(default)]
    pub format: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            language: Lang::En,
            language_chosen: false,
            mode: Mode::Basic,
            theme: Theme::System,
            reduce_motion: false,
            text_percent: 100,
            setup_done: false,
            device: Device::default(),
            hash_cap: 0,
            mining_threads: 0,
            mine_on_start: false,
            auto_update: false,
            auto_boards: Vec::new(),
            nodes: Vec::new(),
            port: NODE_PORT,
            // Others may connect, as Bitcoin Core's `-listen` is on by default.
            listen_lan: true,
            scan_lan: true,
            broadcast: true,
            scan_ports: Vec::new(),
            coin: None,
            peers: Vec::new(),
            added: Vec::new(),
            book: Vec::new(),
            format: FORMAT,
        }
    }
}

/// Copies each named field that differs between `before` and `after` into `into`.
macro_rules! take_changed {
    ($into:expr, $before:expr, $after:expr, $($field:ident),+) => {
        $(if $before.$field != $after.$field {
            $into.$field.clone_from(&$after.$field);
        })+
    };
}

impl Settings {
    /// Takes what the Settings page changed between the copy it had (`before`) and the one it
    /// sends (`after`). A value changed elsewhere since, as by the console, stays.
    pub fn take_changes(&mut self, before: &Self, after: &Self) {
        take_changed!(
            self, before, after, auto_update, auto_boards, language, language_chosen, mode, theme,
            reduce_motion, text_percent, device, hash_cap, mining_threads, mine_on_start, port,
            listen_lan, scan_lan, broadcast, scan_ports
        );
    }

    /// A device says only how many miners to make. Caps and switches stay as a person set them.
    pub fn apply_device(&mut self, device: Device) {
        self.device = device;
    }

    /// The threads that mine: the chosen number, or `standard`, all cores but one.
    pub fn threads(&self, standard: usize) -> usize {
        match self.mining_threads {
            0 => standard,
            chosen => chosen as usize,
        }
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
        saved.unwrap_or_default().moved_to_new_defaults().following(system)
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

    /// An older file holds what its device choice wrote: 20,000 hashes a second, and no
    /// listening on a computer. Neither was a person's choice, so both take today's defaults.
    fn moved_to_new_defaults(mut self) -> Self {
        if self.format < FORMAT {
            if self.hash_cap == OLD_COMPUTER_CAP {
                self.hash_cap = 0;
            }
            if matches!(self.device, Device::ThisComputer | Device::SeveralNodes) {
                self.listen_lan = true;
            }
            self.format = FORMAT;
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
    fn the_update_switch_starts_off_and_is_stored() {
        assert!(!Settings::default().auto_update);
        let mut stored = Settings::default();
        let chosen = Settings {
            auto_update: true,
            auto_boards: vec![[7; 6]],
            ..Settings::default()
        };
        stored.take_changes(&Settings::default(), &chosen);
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
        assert!(settings.listen_lan);
        assert_eq!(Device::Esp32.defaults().local_nodes, 0);
    }

    #[test]
    fn a_save_takes_only_what_the_page_changed() {
        let page = Settings::default();
        let mut core = Settings { hash_cap: 5_000, ..page.clone() };
        core.take_changes(&page, &Settings { theme: Theme::Dark, ..page.clone() });
        assert_eq!(core.hash_cap, 5_000);
        assert_eq!(core.theme, Theme::Dark);
        core.take_changes(&page, &Settings { hash_cap: 7, ..page.clone() });
        assert_eq!(core.hash_cap, 7);
    }

    #[test]
    fn the_steps_are_parts_of_the_full_rate() {
        for full in [8_000, 2_000_000, 40_000_000] {
            assert_eq!(Speed::Quarter.cap(full), full / 4);
            assert_eq!(Speed::Half.cap(full), full / 2);
            assert_eq!(Speed::Full.cap(full), 0);
            assert!(Speed::Quarter.cap(full) < Speed::Half.cap(full));
            assert_eq!(Speed::of(full / 2, full), Some(Speed::Half));
        }
        assert_eq!(Speed::of(12_345, 2_000_000), None);
    }

    #[test]
    fn a_chosen_listen_and_cap_stay_in_a_new_file() {
        let folder = folder_with("new-format", None);
        Settings { hash_cap: 20_000, listen_lan: false, ..Settings::default() }.save(&folder).unwrap();
        let loaded = Settings::load_for(&folder, Lang::En);
        assert_eq!(loaded.hash_cap, 20_000);
        assert!(!loaded.listen_lan);
        fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn others_may_connect_unless_a_person_says_no() {
        assert!(Settings::default().listen_lan, "Bitcoin Core listens by default");
        let mut settings = Settings { listen_lan: false, ..Settings::default() };
        settings.apply_device(Device::Esp32);
        assert!(!settings.listen_lan);
    }

    #[test]
    fn a_device_picked_later_keeps_what_a_person_set() {
        let mut settings = Settings { hash_cap: 900_000, listen_lan: true, ..Settings::default() };
        settings.apply_device(Device::ThisComputer);
        assert_eq!(settings.hash_cap, 900_000);
        assert!(settings.listen_lan);
        settings.apply_device(Device::Esp32);
        assert_eq!(settings.hash_cap, 900_000);
    }

    #[test]
    fn an_old_default_cap_becomes_the_full_speed() {
        let folder = folder_with("old-cap", Some(r#"{"hash_cap":20000,"device":"Esp32","setup_done":true}"#));
        assert_eq!(Settings::load_for(&folder, Lang::En).hash_cap, 0);
        fs::remove_dir_all(&folder).unwrap();
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
