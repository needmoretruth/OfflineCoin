//! Everything a board can be told, by name. The words a board reads stay here, so a screen picks
//! an action and never writes a command.
use olc_control::line::{
    always_quoted, quoted, FEWEST_WIFI_PASSWORD_BYTES, MOST_NAME_BYTES, MOST_WIFI_NAME_BYTES,
    MOST_WIFI_PASSWORD_BYTES,
};

/// What a person can ask of a board. Reading actions leave their answer on the board's row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BoardAction {
    Status,
    RecentStarts,
    ReadScreen,
    ReadKeys,
    ReadCoin,
    ReadPayment(String),
    ReadShares,
    Mine(bool),
    Cap(u64),
    DefaultCap,
    Offline(bool),
    FullSpeed(bool),
    Clock(u32),
    OwnNetwork(bool),
    Wifi { name: String, password: String },
    WifiOff,
    Dial(String),
    DialOff,
    EspNow(bool),
    Bluetooth(bool),
    /// The order links are preferred in.
    Links(String),
    RadioPower(i32),
    Ignore(String),
    HearEveryone,
    View(String),
    Language(String),
    Smooth(u64),
    Bright(u64),
    ShowLogo,
    Name(String),
    Trust(String),
    Pay { to: String, amount: String },
    ClearWaitingPayments,
    MineTo(String),
    MineToOwnKey,
    NewKey,
    UseKey(usize),
    ImportKey(String),
    ExportKey(usize),
    /// Recent blocks to hold, or every block.
    Keep(Option<u64>),
    Merge(bool),
    Note(String),
    MineNote(String),
    MineNoteOff,
    AcceptSnapshot,
    /// A coin as a hex block. It waits for `ConfirmCoin`.
    MoveToCoin(String),
    DefaultCoin,
    ConfirmCoin,
    TakeWork { header: String, difficulty: String },
    StopWork,
    /// Whether the board holds headers only.
    Light(bool),
    /// Who the board hears around it.
    Neighbours,
    ReadWires,
    /// Crosses the board's two wire pins.
    SwapWires,
    WireSpeed(u32),
}

fn on_off(on: bool) -> &'static str {
    if on {
        "on"
    } else {
        "off"
    }
}

/// A value only a program writes, like a key or an address. It has no spaces to keep.
fn word(text: &str) -> Option<&str> {
    let text = text.trim();
    (!text.is_empty() && !text.contains(char::is_whitespace)).then_some(text)
}

fn text(text: &str) -> Option<&str> {
    Some(text.trim()).filter(|text| !text.is_empty() && !text.contains('\n'))
}

/// A network's name in a line. One called `off` must not read as the word that turns Wi-Fi off.
fn wifi_name(name: &str) -> String {
    match name {
        "off" => always_quoted(name),
        _ => quoted(name),
    }
}

fn breaks_a_line(text: &str) -> bool {
    text.contains(['\n', '\r'])
}

impl BoardAction {
    /// The line for the board, or `None` when the values cannot be said in one.
    pub fn line(&self) -> Option<String> {
        Some(match self {
            Self::Status => "status".into(),
            Self::RecentStarts => "boots".into(),
            Self::ReadScreen => "screen".into(),
            Self::ReadKeys => "keys".into(),
            Self::ReadCoin => "coin".into(),
            Self::ReadPayment(id) => format!("tx {}", word(id)?),
            Self::ReadShares => "work shares".into(),
            Self::Mine(on) => format!("mine {}", on_off(*on)),
            Self::Cap(cap) => format!("limit {cap}"),
            Self::DefaultCap => "limit default".into(),
            Self::Offline(on) => format!("offline {}", on_off(*on)),
            Self::FullSpeed(on) => format!("full {}", on_off(*on)),
            Self::Clock(mhz) => format!("clock {mhz}"),
            Self::OwnNetwork(on) => format!("network {}", on_off(*on)),
            Self::Wifi { .. } | Self::Name(_) if self.fault().is_some() => return None,
            Self::Wifi { name, password } if password.is_empty() => format!("wifi {}", wifi_name(name)),
            Self::Wifi { name, password } => format!("wifi {} {}", wifi_name(name), quoted(password)),
            Self::WifiOff => "wifi off".into(),
            Self::Dial(target) => format!("dial {}", word(target)?),
            Self::DialOff => "dial off".into(),
            Self::EspNow(on) => format!("espnow {}", on_off(*on)),
            Self::Bluetooth(on) => format!("bluetooth {}", on_off(*on)),
            Self::Links(order) => format!("links {}", word(order)?),
            Self::RadioPower(dbm) => format!("power {dbm}"),
            Self::Ignore(peer) => format!("ignore {}", word(peer)?),
            Self::HearEveryone => "ignore none".into(),
            Self::View(view) => format!("view {}", word(view)?),
            Self::Language(language) => format!("lang {}", word(language)?),
            Self::Smooth(frames) => format!("smooth {frames}"),
            Self::Bright(level) => format!("bright {level}"),
            Self::ShowLogo => "logo".into(),
            Self::Name(name) => format!("name {}", quoted(name)),
            Self::Trust(key) => format!("trust {}", word(key)?),
            Self::Pay { to, amount } => format!("pay {} {}", word(to)?, word(amount)?),
            Self::ClearWaitingPayments => "pool clear".into(),
            Self::MineTo(address) => format!("mineto {}", word(address)?),
            Self::MineToOwnKey => "mineto own".into(),
            Self::NewKey => "key new".into(),
            Self::UseKey(index) => format!("key use {index}"),
            Self::ImportKey(secret) => format!("key import {}", word(secret)?),
            Self::ExportKey(index) => format!("key export {index}"),
            Self::Keep(Some(blocks)) => format!("keep {blocks}"),
            Self::Keep(None) => "keep all".into(),
            Self::Merge(on) => format!("merge {}", on_off(*on)),
            Self::Note(note) => format!("note {}", text(note)?),
            Self::MineNote(note) => format!("minenote {}", text(note)?),
            Self::MineNoteOff => "minenote off".into(),
            Self::AcceptSnapshot => "snapshot accept".into(),
            Self::MoveToCoin(coin) => format!("coin {}", word(coin)?),
            Self::DefaultCoin => "coin default".into(),
            Self::ConfirmCoin => "coin confirm".into(),
            Self::TakeWork { header, difficulty } => {
                format!("work {} {}", word(header)?, word(difficulty)?)
            }
            Self::StopWork => "work stop".into(),
            Self::Light(on) => format!("light {}", on_off(*on)),
            Self::Neighbours => "near".into(),
            Self::ReadWires => "wires".into(),
            Self::SwapWires => "wires swap".into(),
            Self::WireSpeed(baud) => format!("wires {baud}"),
        })
    }

    /// Reads what `line` writes, so a typed line and a button are one action.
    pub fn parse(words: &[&str]) -> Option<Self> {
        let number = |word: &str| word.parse().ok();
        let on = |word: &str| match word {
            "on" => Some(true),
            "off" => Some(false),
            _ => None,
        };
        let name = |word: &&str| word.to_string();
        let rest = |from: usize| words[from..].join(" ");
        Some(match words {
            ["status"] => Self::Status,
            ["boots"] => Self::RecentStarts,
            ["screen"] => Self::ReadScreen,
            ["keys"] => Self::ReadKeys,
            ["coin"] => Self::ReadCoin,
            ["tx", id] => Self::ReadPayment(name(id)),
            ["work", "shares"] => Self::ReadShares,
            ["work", "stop"] => Self::StopWork,
            ["mine", word] => Self::Mine(on(word)?),
            ["limit", "default"] => Self::DefaultCap,
            ["limit", cap] => Self::Cap(number(cap)?),
            ["offline", word] => Self::Offline(on(word)?),
            ["full", word] => Self::FullSpeed(on(word)?),
            ["clock", mhz] => Self::Clock(mhz.parse().ok()?),
            ["network", word] => Self::OwnNetwork(on(word)?),
            ["wifi", "off"] => Self::WifiOff,
            ["wifi", wifi] => Self::Wifi { name: name(wifi), password: String::new() },
            ["wifi", wifi, password] => Self::Wifi { name: name(wifi), password: name(password) },
            ["dial", "off"] => Self::DialOff,
            ["dial", target] => Self::Dial(name(target)),
            ["espnow", word] => Self::EspNow(on(word)?),
            ["bluetooth", word] => Self::Bluetooth(on(word)?),
            ["links", order] => Self::Links(name(order)),
            ["light", word] => Self::Light(on(word)?),
            ["near"] => Self::Neighbours,
            ["wires"] => Self::ReadWires,
            ["wires", "swap"] => Self::SwapWires,
            ["wires", baud] => Self::WireSpeed(baud.parse().ok()?),
            ["power", dbm] => Self::RadioPower(dbm.parse().ok()?),
            ["ignore", "none"] => Self::HearEveryone,
            ["ignore", peer] => Self::Ignore(name(peer)),
            ["view", view] => Self::View(name(view)),
            ["lang", language] => Self::Language(name(language)),
            ["smooth", frames] => Self::Smooth(number(frames)?),
            ["bright", level] => Self::Bright(number(level)?),
            ["logo"] => Self::ShowLogo,
            ["name", _, ..] => Self::Name(rest(1)),
            ["trust", key] => Self::Trust(name(key)),
            ["pay", to, amount] => Self::Pay { to: name(to), amount: name(amount) },
            ["pool", "clear"] => Self::ClearWaitingPayments,
            ["mineto", "own"] => Self::MineToOwnKey,
            ["mineto", address] => Self::MineTo(name(address)),
            ["key", "new"] => Self::NewKey,
            ["key", "use", index] => Self::UseKey(index.parse().ok()?),
            ["key", "import", secret] => Self::ImportKey(name(secret)),
            ["key", "export", index] => Self::ExportKey(index.parse().ok()?),
            ["keep", "all"] => Self::Keep(None),
            ["keep", blocks] => Self::Keep(Some(number(blocks)?)),
            ["merge", word] => Self::Merge(on(word)?),
            ["minenote", "off"] => Self::MineNoteOff,
            ["note", _, ..] => Self::Note(rest(1)),
            ["minenote", _, ..] => Self::MineNote(rest(1)),
            ["snapshot", "accept"] => Self::AcceptSnapshot,
            ["coin", "default"] => Self::DefaultCoin,
            ["coin", "confirm"] => Self::ConfirmCoin,
            ["coin", text] => Self::MoveToCoin(name(text)),
            ["work", header, difficulty] => {
                Self::TakeWork { header: name(header), difficulty: name(difficulty) }
            }
            _ => return None,
        })
    }

    /// A board takes these only over its cable, so no radio line can touch a key or a network.
    pub fn needs_cable(&self) -> bool {
        matches!(
            self,
            Self::ReadScreen
                | Self::Wifi { .. }
                | Self::WifiOff
                | Self::Dial(_)
                | Self::DialOff
                | Self::EspNow(_)
                | Self::Trust(_)
                | Self::NewKey
                | Self::UseKey(_)
                | Self::ImportKey(_)
                | Self::ExportKey(_)
        )
    }

    /// Why these values cannot go to a board, as the key of a sentence.
    pub fn fault(&self) -> Option<&'static str> {
        match self {
            Self::Name(name) if name.trim().is_empty() || breaks_a_line(name) => {
                Some("notice.name_bad")
            }
            Self::Name(name) if name.len() > MOST_NAME_BYTES => Some("notice.name_too_long"),
            Self::Wifi { name, .. } if name.trim().is_empty() || breaks_a_line(name) => {
                Some("notice.wifi_name_bad")
            }
            Self::Wifi { name, .. } if name.len() > MOST_WIFI_NAME_BYTES => {
                Some("notice.wifi_name_too_long")
            }
            Self::Wifi { password, .. } if breaks_a_line(password) => {
                Some("notice.wifi_password_bad")
            }
            Self::Wifi { password, .. } if password.len() > MOST_WIFI_PASSWORD_BYTES => {
                Some("notice.wifi_password_bad")
            }
            Self::Wifi { password, .. }
                if !password.is_empty() && password.len() < FEWEST_WIFI_PASSWORD_BYTES =>
            {
                Some("notice.wifi_password_bad")
            }
            _ => None,
        }
    }

    /// What a notice may repeat. A secret stays out of it.
    pub fn shown(&self) -> String {
        match self {
            Self::Wifi { name, .. } => format!("wifi {name}"),
            Self::ImportKey(_) => "key import".into(),
            Self::MoveToCoin(_) => "coin".into(),
            other => other.line().unwrap_or_default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use olc_control::allowed_remotely;
    use olc_control::line::{name_in, wifi_in};

    #[test]
    fn a_value_that_would_break_the_line_is_refused() {
        let wifi = |name: &str| BoardAction::Wifi { name: name.into(), password: String::new() };
        assert_eq!(wifi("home").line().as_deref(), Some("wifi home"));
        assert_eq!(wifi("my home").line().as_deref(), Some("wifi \"my home\""));
        assert_eq!(BoardAction::Name(String::new()).line(), None);
        assert_eq!(BoardAction::Note("two words".into()).line().unwrap(), "note two words");
    }

    #[test]
    fn a_name_with_spaces_and_korean_reaches_what_the_board_reads() {
        let line = BoardAction::Name("내 보드 2".into()).line().unwrap();
        assert_eq!(name_in(&line).as_deref(), Some("내 보드 2"));
        assert!(allowed_remotely(&line));

        let joined = BoardAction::Wifi { name: "My Home".into(), password: "pass word".into() };
        let line = joined.line().unwrap();
        assert_eq!(wifi_in(&line), Some(("My Home".into(), "pass word".into())));
        let quote = BoardAction::Wifi { name: "우리 집".into(), password: "a \"b\" \\c  ".into() };
        let parts = wifi_in(&quote.line().unwrap()).unwrap();
        assert_eq!(parts, ("우리 집".into(), "a \"b\" \\c  ".into()));
        let off = BoardAction::Wifi { name: "off".into(), password: String::new() };
        assert_eq!(wifi_in(&off.line().unwrap()), Some(("off".into(), String::new())));
        assert_ne!(off.line().unwrap(), "wifi off");
    }

    #[test]
    fn a_value_a_board_cannot_keep_says_why() {
        let wifi = |name: &str, password: &str| BoardAction::Wifi {
            name: name.into(),
            password: password.into(),
        };
        let long = "가".repeat(11);
        assert_eq!(BoardAction::Name(long.clone()).fault(), Some("notice.name_too_long"));
        assert_eq!(BoardAction::Name("  ".into()).fault(), Some("notice.name_bad"));
        assert_eq!(wifi(&long, "").fault(), Some("notice.wifi_name_too_long"));
        assert_eq!(wifi("home", "short").fault(), Some("notice.wifi_password_bad"));
        assert_eq!(wifi("home", &"x".repeat(64)).fault(), Some("notice.wifi_password_bad"));
        assert_eq!(wifi("home", "").fault(), None);
        assert_eq!(wifi("home", "eight 8 ").fault(), None);
        assert_eq!(wifi("home", "short").line(), None);
    }

    #[test]
    fn a_notice_keeps_secrets_out() {
        let wifi = BoardAction::Wifi { name: "home".into(), password: "secret12".into() };
        assert_eq!(wifi.line().unwrap(), "wifi home secret12");
        assert!(!wifi.shown().contains("secret"));
        assert!(!BoardAction::ImportKey("abcd".into()).shown().contains("abcd"));
    }

    #[test]
    fn cable_only_is_exactly_what_the_radio_gate_refuses() {
        let word = || "a".to_string();
        let actions = [
            BoardAction::Status,
            BoardAction::RecentStarts,
            BoardAction::ReadScreen,
            BoardAction::ReadKeys,
            BoardAction::ReadCoin,
            BoardAction::ReadPayment(word()),
            BoardAction::ReadShares,
            BoardAction::Mine(true),
            BoardAction::Cap(1),
            BoardAction::DefaultCap,
            BoardAction::Offline(true),
            BoardAction::FullSpeed(true),
            BoardAction::Clock(80),
            BoardAction::OwnNetwork(true),
            BoardAction::Wifi { name: word(), password: "password".into() },
            BoardAction::WifiOff,
            BoardAction::Dial(word()),
            BoardAction::DialOff,
            BoardAction::EspNow(true),
            BoardAction::RadioPower(10),
            BoardAction::Ignore(word()),
            BoardAction::HearEveryone,
            BoardAction::View(word()),
            BoardAction::Language(word()),
            BoardAction::Smooth(30),
            BoardAction::Bright(1),
            BoardAction::ShowLogo,
            BoardAction::Name(word()),
            BoardAction::Trust(word()),
            BoardAction::Pay { to: word(), amount: word() },
            BoardAction::ClearWaitingPayments,
            BoardAction::MineTo(word()),
            BoardAction::MineToOwnKey,
            BoardAction::NewKey,
            BoardAction::UseKey(0),
            BoardAction::ImportKey(word()),
            BoardAction::ExportKey(0),
            BoardAction::Keep(None),
            BoardAction::Merge(true),
            BoardAction::Note(word()),
            BoardAction::MineNote(word()),
            BoardAction::MineNoteOff,
            BoardAction::AcceptSnapshot,
            BoardAction::MoveToCoin(word()),
            BoardAction::DefaultCoin,
            BoardAction::ConfirmCoin,
            BoardAction::TakeWork { header: word(), difficulty: word() },
            BoardAction::StopWork,
            BoardAction::Bluetooth(true),
            BoardAction::Links(word()),
            BoardAction::Light(true),
            BoardAction::Neighbours,
            BoardAction::ReadWires,
            BoardAction::SwapWires,
            BoardAction::WireSpeed(115_200),
        ];
        for action in actions {
            let line = action.line().unwrap();
            assert_eq!(action.needs_cable(), !allowed_remotely(&line), "{line}");
            let words: Vec<&str> = line.split(' ').collect();
            assert_eq!(BoardAction::parse(&words).as_ref(), Some(&action), "{line}");
        }
    }
}
