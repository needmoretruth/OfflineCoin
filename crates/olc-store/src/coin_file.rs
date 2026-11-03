//! Coin files: the JSON form of a coin, with addresses written as text.
use olc_core::coin::{Fork, UNSAID};
use olc_core::{CoinConfig, Rules};
use olc_wallet::{format_address, parse_address};
use serde::{Deserialize, Serialize};

pub use crate::coin_fields::{new_coin, NewCoin};

#[derive(Serialize, Deserialize)]
struct Premine {
    address: String,
    amount: u64,
}

#[derive(Serialize, Deserialize)]
struct CoinFile {
    name: String,
    ticker: String,
    message: String,
    genesis_time_ms: u64,
    initial_difficulty: u64,
    premine: Vec<Premine>,
    rules: Rules,
    #[serde(default)]
    message_in_block: bool,
    /// Left out when there are none, so a coin file stays what it was.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    forks: Vec<Fork>,
    /// The upgrade height, left out for a coin from before it. See [`CoinConfig::upgrade_from`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    upgrade_from: Option<u64>,
    /// Whether blocks carry Bitcoin's header, left out for a coin from before it.
    #[serde(default, skip_serializing_if = "is_false")]
    bitcoin_header: bool,
}

fn is_false(value: &bool) -> bool {
    !value
}

impl From<&CoinConfig> for CoinFile {
    fn from(coin: &CoinConfig) -> Self {
        let premine = coin
            .premine
            .iter()
            .map(|(address, amount)| Premine {
                address: format_address(address),
                amount: *amount,
            })
            .collect();
        Self {
            name: coin.name.clone(),
            ticker: coin.ticker.clone(),
            message: coin.message.clone(),
            genesis_time_ms: coin.genesis_time_ms,
            initial_difficulty: coin.initial_difficulty,
            premine,
            rules: coin.rules.clone(),
            message_in_block: coin.message_in_block,
            forks: coin.forks.clone(),
            upgrade_from: coin.upgrade_said(),
            bitcoin_header: coin.bitcoin_header,
        }
    }
}

/// A fork starts above the genesis, and two forks do not start together.
fn check_forks(forks: &[Fork]) -> Result<(), String> {
    let mut last = 0;
    let mut heights: Vec<u64> = forks.iter().map(|fork| fork.from_height).collect();
    heights.sort_unstable();
    for height in heights {
        if height <= last {
            return Err(format!("no fork can start at height {height}"));
        }
        last = height;
    }
    Ok(())
}

pub fn from_json(text: &str) -> Result<CoinConfig, String> {
    let file: CoinFile = serde_json::from_str(text).map_err(|error| error.to_string())?;
    let mut premine = Vec::new();
    for entry in file.premine {
        let address = parse_address(&entry.address)
            .ok_or_else(|| format!("bad address {}", entry.address))?;
        premine.push((address, entry.amount));
    }
    check_forks(&file.forks)?;
    let mut coin = CoinConfig {
        name: file.name,
        ticker: file.ticker,
        message: file.message,
        genesis_time_ms: file.genesis_time_ms,
        initial_difficulty: file.initial_difficulty,
        premine,
        rules: file.rules,
        message_in_block: file.message_in_block,
        forks: file.forks,
        upgrade_from: file.upgrade_from.unwrap_or(UNSAID),
        bitcoin_header: file.bitcoin_header,
    };
    coin.settle_floor();
    Ok(coin)
}

pub fn to_json(coin: &CoinConfig) -> Result<String, String> {
    serde_json::to_string_pretty(&CoinFile::from(coin)).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_coin_survives_the_file() {
        let mut coin = CoinConfig::olc();
        coin.premine.push(([7; 20], 5));
        coin.rules.block_time_ms = 1234;
        assert_eq!(from_json(&to_json(&coin).unwrap()).unwrap(), coin);
    }

    fn without_floor(coin: &CoinConfig) -> String {
        let mut file: serde_json::Value = serde_json::from_str(&to_json(coin).unwrap()).unwrap();
        let rules = file["rules"].as_object_mut().unwrap();
        assert!(rules.remove("min_difficulty").is_some());
        assert!(rules.remove("floor_from_height").is_some());
        file.to_string()
    }

    #[test]
    fn a_file_from_before_floors_still_loads() {
        // Every coin from before floors, the first OLC among them, came before OLC's new genesis.
        let mut other = CoinConfig::olc();
        other.name = "Older".into();
        let loaded = from_json(&without_floor(&other)).unwrap();
        assert_eq!(loaded.rules.min_difficulty, 1, "that coin never had a floor");
    }

    #[test]
    fn a_file_from_before_the_choice_keeps_bitcoins_rule_and_header() {
        let mut file: serde_json::Value = serde_json::from_str(&to_json(&CoinConfig::olc()).unwrap()).unwrap();
        let rules = file["rules"].as_object_mut().unwrap();
        for key in ["difficulty_rule", "half_life_blocks", "median_time_blocks", "max_ahead_ms"] {
            assert!(rules.remove(key).is_some(), "{key}");
        }
        assert!(file.as_object_mut().unwrap().remove("bitcoin_header").is_some());
        let loaded = from_json(&file.to_string()).unwrap();
        assert_eq!(loaded.rules.timing(), olc_core::timing::LEGACY);
        assert!(!loaded.bitcoin_header);
        assert_eq!(CoinConfig::olc().rules.difficulty_rule, olc_core::DifficultyRule::Asert);
    }

    #[test]
    fn a_chosen_floor_survives_the_file() {
        let mut coin = CoinConfig::olc();
        coin.name = "Mine".into();
        coin.rules.min_difficulty = 12_345;
        assert_eq!(from_json(&to_json(&coin).unwrap()).unwrap(), coin);
    }
}
