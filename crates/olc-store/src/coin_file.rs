//! Coin files: the JSON form of a coin, with addresses written as text.
use olc_core::{CoinConfig, Rules};
use olc_wallet::{format_address, parse_address};
use serde::{Deserialize, Serialize};

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
        }
    }
}

pub fn from_json(text: &str) -> Result<CoinConfig, String> {
    let file: CoinFile = serde_json::from_str(text).map_err(|error| error.to_string())?;
    let mut premine = Vec::new();
    for entry in file.premine {
        let address = parse_address(&entry.address)
            .ok_or_else(|| format!("bad address {}", entry.address))?;
        premine.push((address, entry.amount));
    }
    Ok(CoinConfig {
        name: file.name,
        ticker: file.ticker,
        message: file.message,
        genesis_time_ms: file.genesis_time_ms,
        initial_difficulty: file.initial_difficulty,
        premine,
        rules: file.rules,
    })
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
}
