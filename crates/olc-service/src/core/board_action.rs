//! Everything a board can be told, by name. The words a board reads stay here, so a screen picks
//! an action and never writes a command.

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
}

fn on_off(on: bool) -> &'static str {
    if on {
        "on"
    } else {
        "off"
    }
}

/// A board splits its lines at spaces, so one value must be one word.
fn word(text: &str) -> Option<&str> {
    let text = text.trim();
    (!text.is_empty() && !text.contains(char::is_whitespace)).then_some(text)
}

fn text(text: &str) -> Option<&str> {
    Some(text.trim()).filter(|text| !text.is_empty() && !text.contains('\n'))
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
            Self::Wifi { name, password } if password.is_empty() => format!("wifi {}", word(name)?),
            Self::Wifi { name, password } => format!("wifi {} {}", word(name)?, word(password)?),
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
            Self::Name(name) => format!("name {}", word(name)?),
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
            ["power", dbm] => Self::RadioPower(dbm.parse().ok()?),
            ["ignore", "none"] => Self::HearEveryone,
            ["ignore", peer] => Self::Ignore(name(peer)),
            ["view", view] => Self::View(name(view)),
            ["lang", language] => Self::Language(name(language)),
            ["smooth", frames] => Self::Smooth(number(frames)?),
            ["bright", level] => Self::Bright(number(level)?),
            ["logo"] => Self::ShowLogo,
            ["name", board] => Self::Name(name(board)),
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

    #[test]
    fn a_value_that_would_break_the_line_is_refused() {
        let wifi = |name: &str| BoardAction::Wifi { name: name.into(), password: String::new() };
        assert_eq!(wifi("home").line().as_deref(), Some("wifi home"));
        assert_eq!(wifi("my home").line(), None);
        assert_eq!(BoardAction::Name(String::new()).line(), None);
        assert_eq!(BoardAction::Note("two words".into()).line().unwrap(), "note two words");
    }

    #[test]
    fn a_notice_keeps_secrets_out() {
        let wifi = BoardAction::Wifi { name: "home".into(), password: "secret".into() };
        assert_eq!(wifi.line().unwrap(), "wifi home secret");
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
            BoardAction::Wifi { name: word(), password: word() },
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
        ];
        for action in actions {
            let line = action.line().unwrap();
            assert_eq!(action.needs_cable(), !allowed_remotely(&line), "{line}");
            let words: Vec<&str> = line.split(' ').collect();
            assert_eq!(BoardAction::parse(&words).as_ref(), Some(&action), "{line}");
        }
    }
}
