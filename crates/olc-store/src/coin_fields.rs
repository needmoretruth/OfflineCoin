//! Every setting of a coin, in one table. The form, `olc new-coin` and a rule change all read it,
//! so a setting cannot be reachable from one and not from another.
use crate::amount::{format_coins, parse_coins};
use core::fmt;
use olc_core::coin::Fork;
use olc_core::{Address, CoinConfig, DifficultyRule, Rules};
use olc_wallet::{format_address, parse_address};

/// What a board's flash keeps of a coin. Past this a coin can never move to a board.
pub const MOST_COIN_BYTES: usize = 4_000;
/// Below this the block a miner builds has no room for its own reward.
pub const FEWEST_BLOCK_BYTES: u64 = 256;
/// A block travels in one message, and a link drops a message over 8 MiB.
pub const MOST_BLOCK_BYTES: u64 = 8 * 1024 * 1024 - 4_096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldKind {
    Text,
    Number,
    Coins,
    Switch,
    Premine,
    /// One of a few names, as the file writes them.
    Choice(&'static [&'static str]),
}

/// Why a value cannot be used, for a person to fix.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Why {
    Empty,
    NotNumber,
    NotAmount,
    NotSwitch,
    NotOneOf(&'static [&'static str]),
    AtLeast(u64),
    AtMost(u64),
    BadPremine(String),
    TooLong,
    NoChange,
    NotARule,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bad {
    /// The field it belongs to.
    pub key: String,
    pub why: Why,
}

impl Bad {
    fn on(key: &str, why: Why) -> Self {
        Self { key: key.to_string(), why }
    }
}

impl fmt::Display for Bad {
    fn fmt(&self, out: &mut fmt::Formatter) -> fmt::Result {
        let key = &self.key;
        match &self.why {
            Why::Empty => write!(out, "{key} cannot be empty."),
            Why::NotNumber => write!(out, "{key} needs a whole number."),
            Why::NotAmount => write!(out, "{key} needs an amount, like 12.5."),
            Why::NotSwitch => write!(out, "{key} is on or off."),
            Why::NotOneOf(names) => write!(out, "{key} is one of {}.", names.join(", ")),
            Why::AtLeast(least) => write!(out, "{key} must be at least {least}."),
            Why::AtMost(most) => write!(out, "{key} can be at most {most}."),
            Why::BadPremine(line) => write!(out, "\"{line}\" is not an address and an amount."),
            Why::TooLong => write!(out, "The coin is too long for a board to keep."),
            Why::NoChange => write!(out, "No rule changed."),
            Why::NotARule => write!(out, "{key} is not a rule, so it cannot change after the start."),
        }
    }
}

pub struct CoinField {
    /// Its name in a coin file, with `rules.` for what sits in the rules.
    pub key: &'static str,
    /// Its `--flag` on `olc new-coin`.
    pub flag: &'static str,
    pub kind: FieldKind,
    pub get: fn(&CoinConfig) -> String,
    pub set: fn(&mut CoinConfig, &str) -> Result<(), Why>,
}

impl CoinField {
    /// A rule can change by a fork. The rest made the genesis, which never changes.
    pub fn is_rule(&self) -> bool {
        self.key.starts_with("rules.")
    }
}

fn number(text: &str) -> Result<u64, Why> {
    text.trim().replace(',', "").parse().map_err(|_| Why::NotNumber)
}

fn at_least(text: &str, least: u64) -> Result<u64, Why> {
    let value = number(text)?;
    if value < least {
        return Err(Why::AtLeast(least));
    }
    Ok(value)
}

fn coins(text: &str) -> Result<u64, Why> {
    parse_coins(text).ok_or(Why::NotAmount)
}

fn words(text: &str) -> Result<String, Why> {
    match text.trim() {
        "" => Err(Why::Empty),
        text => Ok(text.to_string()),
    }
}

fn switch(text: &str) -> Result<bool, Why> {
    match text.trim() {
        "on" => Ok(true),
        "off" => Ok(false),
        _ => Err(Why::NotSwitch),
    }
}

/// The difficulty rules by the names a coin file gives them, in the order the docs compare them.
pub const DIFFICULTY_RULES: [&str; 3] = ["bitcoin", "bip94", "asert"];

fn rule_name(rule: DifficultyRule) -> &'static str {
    match rule {
        DifficultyRule::Bitcoin => DIFFICULTY_RULES[0],
        DifficultyRule::Bip94 => DIFFICULTY_RULES[1],
        DifficultyRule::Asert => DIFFICULTY_RULES[2],
    }
}

fn rule_from(text: &str) -> Result<DifficultyRule, Why> {
    match text.trim() {
        "bitcoin" => Ok(DifficultyRule::Bitcoin),
        "bip94" => Ok(DifficultyRule::Bip94),
        "asert" => Ok(DifficultyRule::Asert),
        _ => Err(Why::NotOneOf(&DIFFICULTY_RULES)),
    }
}

fn plain(value: u64) -> String {
    value.to_string()
}

fn premine_text(coin: &CoinConfig) -> String {
    let lines = coin.premine.iter().map(|(address, units)| {
        format!("{} {}", format_address(address), format_coins(*units))
    });
    lines.collect::<Vec<_>>().join("; ")
}

/// `ADDRESS AMOUNT`, one for each person, split by a line or `;`.
fn premine_from(text: &str) -> Result<Vec<(Address, u64)>, Why> {
    let lines = text.split([';', '\n']).map(str::trim).filter(|line| !line.is_empty());
    let mut list = Vec::new();
    let mut total = 0u64;
    for line in lines {
        let bad = || Why::BadPremine(line.to_string());
        let (address, amount) = line.split_once(char::is_whitespace).ok_or_else(bad)?;
        let address = parse_address(address.trim()).ok_or_else(bad)?;
        let units = parse_coins(amount).ok_or_else(bad)?;
        total = total.checked_add(units).ok_or_else(bad)?;
        list.push((address, units));
    }
    Ok(list)
}

pub const COIN_FIELDS: &[CoinField] = &[
    CoinField {
        key: "name",
        flag: "name",
        kind: FieldKind::Text,
        get: |coin| coin.name.clone(),
        set: |coin, text| {
            coin.name = words(text)?;
            Ok(())
        },
    },
    CoinField {
        key: "ticker",
        flag: "ticker",
        kind: FieldKind::Text,
        get: |coin| coin.ticker.clone(),
        set: |coin, text| {
            coin.ticker = words(text)?;
            Ok(())
        },
    },
    CoinField {
        key: "message",
        flag: "message",
        kind: FieldKind::Text,
        get: |coin| coin.message.clone(),
        set: |coin, text| {
            coin.message = words(text)?;
            Ok(())
        },
    },
    CoinField {
        key: "message_in_block",
        flag: "message-in-block",
        kind: FieldKind::Switch,
        get: |coin| if coin.message_in_block { "on" } else { "off" }.to_string(),
        set: |coin, text| {
            coin.message_in_block = switch(text)?;
            Ok(())
        },
    },
    CoinField {
        key: "genesis_time_ms",
        flag: "genesis-time-ms",
        kind: FieldKind::Number,
        get: |coin| plain(coin.genesis_time_ms),
        set: |coin, text| {
            coin.genesis_time_ms = number(text)?;
            Ok(())
        },
    },
    CoinField {
        key: "initial_difficulty",
        flag: "difficulty",
        kind: FieldKind::Number,
        get: |coin| plain(coin.initial_difficulty),
        set: |coin, text| {
            coin.initial_difficulty = at_least(text, 1)?;
            Ok(())
        },
    },
    CoinField {
        key: "premine",
        flag: "premine",
        kind: FieldKind::Premine,
        get: premine_text,
        set: |coin, text| {
            coin.premine = premine_from(text)?;
            Ok(())
        },
    },
    CoinField {
        key: "rules.block_time_ms",
        flag: "block-time-ms",
        kind: FieldKind::Number,
        get: |coin| plain(coin.rules.block_time_ms),
        set: |coin, text| {
            coin.rules.block_time_ms = at_least(text, 1)?;
            Ok(())
        },
    },
    CoinField {
        key: "rules.retarget_interval",
        flag: "retarget",
        kind: FieldKind::Number,
        get: |coin| plain(coin.rules.retarget_interval),
        set: |coin, text| {
            coin.rules.retarget_interval = number(text)?;
            Ok(())
        },
    },
    CoinField {
        key: "rules.reward",
        flag: "reward",
        kind: FieldKind::Coins,
        get: |coin| format_coins(coin.rules.reward),
        set: |coin, text| {
            coin.rules.reward = coins(text)?;
            Ok(())
        },
    },
    CoinField {
        key: "rules.halving_interval",
        flag: "halving",
        kind: FieldKind::Number,
        get: |coin| plain(coin.rules.halving_interval),
        set: |coin, text| {
            coin.rules.halving_interval = number(text)?;
            Ok(())
        },
    },
    CoinField {
        key: "rules.coinbase_maturity",
        flag: "maturity",
        kind: FieldKind::Number,
        get: |coin| plain(coin.rules.coinbase_maturity),
        set: |coin, text| {
            coin.rules.coinbase_maturity = number(text)?;
            Ok(())
        },
    },
    CoinField {
        key: "rules.max_block_bytes",
        flag: "max-block-bytes",
        kind: FieldKind::Number,
        get: |coin| plain(coin.rules.max_block_bytes),
        set: |coin, text| {
            let value = at_least(text, FEWEST_BLOCK_BYTES)?;
            if value > MOST_BLOCK_BYTES {
                return Err(Why::AtMost(MOST_BLOCK_BYTES));
            }
            coin.rules.max_block_bytes = value;
            Ok(())
        },
    },
    CoinField {
        key: "rules.min_fee",
        flag: "min-fee",
        kind: FieldKind::Coins,
        get: |coin| format_coins(coin.rules.min_fee),
        set: |coin, text| {
            coin.rules.min_fee = coins(text)?;
            Ok(())
        },
    },
    CoinField {
        key: "rules.min_difficulty",
        flag: "min-difficulty",
        kind: FieldKind::Number,
        get: |coin| plain(coin.rules.min_difficulty),
        set: |coin, text| {
            coin.rules.min_difficulty = at_least(text, 1)?;
            Ok(())
        },
    },
    CoinField {
        key: "rules.floor_from_height",
        flag: "floor-from-height",
        kind: FieldKind::Number,
        get: |coin| plain(coin.rules.floor_from_height),
        set: |coin, text| {
            coin.rules.floor_from_height = number(text)?;
            Ok(())
        },
    },
    CoinField {
        key: "rules.difficulty_rule",
        flag: "difficulty-rule",
        kind: FieldKind::Choice(&DIFFICULTY_RULES),
        get: |coin| rule_name(coin.rules.difficulty_rule).to_string(),
        set: |coin, text| {
            coin.rules.difficulty_rule = rule_from(text)?;
            Ok(())
        },
    },
    CoinField {
        key: "rules.half_life_blocks",
        flag: "half-life",
        kind: FieldKind::Number,
        get: |coin| plain(coin.rules.half_life_blocks),
        set: |coin, text| {
            coin.rules.half_life_blocks = at_least(text, 1)?;
            Ok(())
        },
    },
    CoinField {
        key: "rules.allow_min_difficulty_blocks",
        flag: "min-difficulty-blocks",
        kind: FieldKind::Switch,
        get: |coin| if coin.rules.allow_min_difficulty_blocks { "on" } else { "off" }.to_string(),
        set: |coin, text| {
            coin.rules.allow_min_difficulty_blocks = switch(text)?;
            Ok(())
        },
    },
    CoinField {
        key: "rules.median_time_blocks",
        flag: "median-time",
        kind: FieldKind::Number,
        get: |coin| plain(coin.rules.median_time_blocks),
        set: |coin, text| {
            coin.rules.median_time_blocks = number(text)?;
            Ok(())
        },
    },
    CoinField {
        key: "rules.max_ahead_ms",
        flag: "max-ahead-ms",
        kind: FieldKind::Number,
        get: |coin| plain(coin.rules.max_ahead_ms),
        set: |coin, text| {
            coin.rules.max_ahead_ms = number(text)?;
            Ok(())
        },
    },
    CoinField {
        key: "bitcoin_header",
        flag: "bitcoin-header",
        kind: FieldKind::Switch,
        get: |coin| if coin.bitcoin_header { "on" } else { "off" }.to_string(),
        set: |coin, text| {
            coin.bitcoin_header = switch(text)?;
            Ok(())
        },
    },
];

pub fn coin_field(key: &str) -> Option<&'static CoinField> {
    COIN_FIELDS.iter().find(|field| field.key == key)
}

/// The choices behind a new coin, as a person typed them: a field's key and its text. Empty text
/// and what is left out stay as OfflineCoin has them, or follow from the rest.
#[derive(Clone, Debug, Default)]
pub struct NewCoin {
    pub values: Vec<(String, String)>,
}

impl NewCoin {
    pub fn with(mut self, key: &str, text: impl Into<String>) -> Self {
        self.values.push((key.to_string(), text.into()));
        self
    }

    fn given(&self, key: &str) -> bool {
        self.values.iter().any(|(named, text)| named == key && !text.trim().is_empty())
    }
}

fn fits_a_board(coin: &CoinConfig) -> Result<(), Bad> {
    match coin.to_bytes().len() > MOST_COIN_BYTES {
        true => Err(Bad::on("message", Why::TooLong)),
        false => Ok(()),
    }
}

/// One way to make a coin, for the window and the command line alike.
pub fn new_coin(choices: &NewCoin) -> Result<CoinConfig, Bad> {
    let mut coin = CoinConfig::olc();
    coin.rules.floor_from_height = 0;
    coin.message_in_block = true;
    // A new coin takes the upgrade from its genesis: its signatures name it from the first block.
    coin.upgrade_from = 0;
    for (key, text) in choices.values.iter().filter(|(_, text)| !text.trim().is_empty()) {
        let field = coin_field(key).ok_or_else(|| Bad::on(key, Why::NotARule))?;
        (field.set)(&mut coin, text).map_err(|why| Bad::on(key, why))?;
    }
    if !choices.given("message") && choices.given("name") {
        coin.message = format!("{} genesis", coin.name);
    }
    if !choices.given("rules.min_difficulty") {
        coin.rules.min_difficulty = Rules::floor_for(coin.rules.block_time_ms).max(1);
    }
    if !choices.given("initial_difficulty") {
        coin.initial_difficulty = coin.rules.min_difficulty;
    }
    fits_a_board(&coin)?;
    Ok(coin)
}

/// The lowest height a new fork of `coin` can start at, when the chain stands at `tip`. A block
/// already made was judged by the rules before it.
pub fn first_fork_height(coin: &CoinConfig, tip: u64) -> u64 {
    let last = coin.forks.last().map_or(0, |fork| fork.from_height);
    tip.max(last) + 1
}

/// `coin` with new rules from `from_height`, on the same genesis and history. `changes` are field
/// keys and their text. Only rules can change, since the rest made the genesis.
pub fn fork_coin(
    coin: &CoinConfig,
    from_height: u64,
    tip: u64,
    changes: &[(String, String)],
) -> Result<CoinConfig, Bad> {
    let first = first_fork_height(coin, tip);
    if from_height < first {
        return Err(Bad::on("from_height", Why::AtLeast(first)));
    }
    let mut draft = coin.clone();
    draft.rules = coin.rules_at(from_height).clone();
    for (key, text) in changes {
        let field = coin_field(key).filter(|field| field.is_rule());
        let field = field.ok_or_else(|| Bad::on(key, Why::NotARule))?;
        (field.set)(&mut draft, text).map_err(|why| Bad::on(key, why))?;
    }
    if draft.rules == *coin.rules_at(from_height) {
        return Err(Bad::on("rules", Why::NoChange));
    }
    let mut forked = coin.clone();
    forked.forks.push(Fork { from_height, rules: draft.rules });
    fits_a_board(&forked)?;
    Ok(forked)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coin_file::{from_json, to_json};
    use olc_core::coin::COIN;

    const ADDRESS: [u8; 20] = [7; 20];

    fn samples() -> Vec<(&'static str, String)> {
        let premine = format!("{} 12.5", format_address(&ADDRESS));
        vec![
            ("name", "Walk Coin".to_string()),
            ("ticker", "WLK".to_string()),
            ("message", "우리 코인 첫 블록".to_string()),
            ("message_in_block", "off".to_string()),
            ("genesis_time_ms", "1234567".to_string()),
            ("initial_difficulty", "321".to_string()),
            ("premine", premine),
            ("rules.block_time_ms", "200".to_string()),
            ("rules.retarget_interval", "7".to_string()),
            ("rules.reward", "0.25".to_string()),
            ("rules.halving_interval", "11".to_string()),
            ("rules.coinbase_maturity", "13".to_string()),
            ("rules.max_block_bytes", "9000".to_string()),
            ("rules.min_fee", "0.001".to_string()),
            ("rules.min_difficulty", "17".to_string()),
            ("rules.floor_from_height", "19".to_string()),
            ("rules.difficulty_rule", "bip94".to_string()),
            ("rules.half_life_blocks", "23".to_string()),
            ("rules.allow_min_difficulty_blocks", "off".to_string()),
            ("rules.median_time_blocks", "5".to_string()),
            ("rules.max_ahead_ms", "60000".to_string()),
            ("bitcoin_header", "off".to_string()),
        ]
    }

    fn keys_of(value: &serde_json::Value, before: &str, keys: &mut Vec<String>) {
        for (name, inner) in value.as_object().into_iter().flatten() {
            let key = format!("{before}{name}");
            match inner.is_object() {
                true => keys_of(inner, &format!("{key}."), keys),
                false => keys.push(key),
            }
        }
    }

    #[test]
    fn every_field_of_a_coin_is_in_the_table_and_sets_what_it_says() {
        let mut forked = CoinConfig::olc();
        forked.forks.push(Fork { from_height: 1, rules: Rules::default() });
        let file: serde_json::Value = serde_json::from_str(&to_json(&forked).unwrap()).unwrap();
        let mut keys = Vec::new();
        keys_of(&file, "", &mut keys);
        // A fork starts from a running coin, so it has its own word and a test of real nodes.
        // A new coin takes the upgrade from genesis, so nobody chooses its height.
        keys.retain(|key| key != "forks" && key != "upgrade_from");
        assert_eq!(keys.len(), COIN_FIELDS.len(), "{keys:?}");
        let samples = samples();
        for key in keys {
            let field = coin_field(&key).unwrap_or_else(|| panic!("{key} cannot be set"));
            let sample = samples.iter().find(|(named, _)| *named == key).unwrap();
            let start = new_coin(&NewCoin::default()).unwrap();
            let before = (field.get)(&start);
            let mut coin = start.clone();
            (field.set)(&mut coin, &sample.1).unwrap();
            assert_ne!((field.get)(&coin), before, "{key} set nothing");
            assert_eq!((field.get)(&coin), sample.1, "{key} reads back as written");
            let again = from_json(&to_json(&coin).unwrap()).unwrap();
            assert_eq!((field.get)(&again), sample.1, "{key} is lost in the file");
        }
    }

    #[test]
    fn a_new_coin_takes_every_field_at_once() {
        let choices = samples().iter().fold(NewCoin::default(), |all, (key, text)| all.with(key, text));
        let coin = new_coin(&choices).unwrap();
        for (key, text) in samples() {
            assert_eq!((coin_field(key).unwrap().get)(&coin), text);
        }
        assert_eq!(coin.rules.reward, COIN / 4);
    }

    #[test]
    fn what_is_left_out_follows_from_the_rest() {
        let coin = new_coin(&NewCoin::default().with("name", "Fast").with("rules.block_time_ms", "200")).unwrap();
        assert_eq!(coin.message, "Fast genesis");
        assert_eq!(coin.rules.min_difficulty, 200);
        assert_eq!(coin.initial_difficulty, 200);
        assert_eq!(coin.rules.floor_from_height, 0);
        let day = new_coin(&NewCoin::default().with("rules.block_time_ms", "86400000")).unwrap();
        assert_eq!(day.rules.min_difficulty, 86_400_000);
    }

    #[test]
    fn bad_values_say_what_is_wrong() {
        let bad = |key: &str, text: &str| new_coin(&NewCoin::default().with(key, text)).unwrap_err().why;
        assert_eq!(bad("rules.block_time_ms", "0"), Why::AtLeast(1));
        assert_eq!(bad("rules.reward", "many"), Why::NotAmount);
        assert_eq!(bad("rules.max_block_bytes", "100"), Why::AtLeast(FEWEST_BLOCK_BYTES));
        assert_eq!(bad("rules.max_block_bytes", "99999999"), Why::AtMost(MOST_BLOCK_BYTES));
        assert_eq!(bad("rules.halving_interval", "x"), Why::NotNumber);
        assert_eq!(bad("message", &"long ".repeat(1_000)), Why::TooLong);
        assert!(matches!(bad("premine", "nobody 5"), Why::BadPremine(_)));
    }

    #[test]
    fn a_fork_changes_rules_only_and_only_ahead_of_the_chain() {
        let coin = CoinConfig::olc();
        let change = |key: &str, text: &str| vec![(key.to_string(), text.to_string())];
        let forked = fork_coin(&coin, 10, 9, &change("rules.reward", "7")).unwrap();
        assert_eq!(forked.genesis().id(), coin.genesis().id());
        assert_eq!(forked.rules_at(9).reward, coin.rules.reward);
        assert_eq!(forked.rules_at(10).reward, 7 * COIN);
        let again = fork_coin(&forked, 20, 15, &change("rules.min_fee", "1")).unwrap();
        assert_eq!(again.rules_at(30).reward, 7 * COIN, "a later fork starts from the rules before it");
        assert_eq!(again.before_last_fork(), Some(forked.clone()));

        let bad = fork_coin(&coin, 9, 9, &change("rules.reward", "7")).unwrap_err();
        assert_eq!((bad.key.as_str(), bad.why), ("from_height", Why::AtLeast(10)));
        let bad = fork_coin(&coin, 10, 9, &change("name", "Other")).unwrap_err();
        assert_eq!(bad.why, Why::NotARule);
        let same = format_coins(coin.rules.reward);
        assert_eq!(fork_coin(&coin, 10, 9, &change("rules.reward", &same)).unwrap_err().why, Why::NoChange);
        assert_eq!(fork_coin(&forked, 10, 9, &change("rules.reward", "8")).unwrap_err().why, Why::AtLeast(11));
    }

    #[test]
    fn a_fork_survives_the_file_and_a_plain_coin_writes_no_more_than_before() {
        let coin = CoinConfig::olc();
        let forked = fork_coin(&coin, 10, 9, &[("rules.reward".into(), "7".into())]).unwrap();
        assert_eq!(from_json(&to_json(&forked).unwrap()).unwrap(), forked);
        assert!(!to_json(&coin).unwrap().contains("forks"));
    }
}
