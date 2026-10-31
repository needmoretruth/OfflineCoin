//! Coin files: the JSON form of a coin, with addresses written as text.
use crate::args::Args;
use olc_core::coin::COIN;
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

impl From<CoinConfig> for CoinFile {
    fn from(coin: CoinConfig) -> Self {
        let premine = coin
            .premine
            .iter()
            .map(|(address, amount)| Premine {
                address: format_address(address),
                amount: *amount,
            })
            .collect();
        Self {
            name: coin.name,
            ticker: coin.ticker,
            message: coin.message,
            genesis_time_ms: coin.genesis_time_ms,
            initial_difficulty: coin.initial_difficulty,
            premine,
            rules: coin.rules,
        }
    }
}

pub fn load(path: Option<&str>) -> Result<CoinConfig, String> {
    let Some(path) = path else {
        return Ok(CoinConfig::olc());
    };
    let text = std::fs::read_to_string(path).map_err(|error| format!("{path}: {error}"))?;
    let file: CoinFile = serde_json::from_str(&text).map_err(|error| format!("{path}: {error}"))?;
    let mut premine = Vec::new();
    for entry in file.premine {
        let address = parse_address(&entry.address)
            .ok_or_else(|| format!("{path}: bad address {}", entry.address))?;
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

pub fn create(args: &Args) -> Result<(), String> {
    let path = args
        .word(1)
        .ok_or("Name the file: olc new-coin mycoin.json")?;
    let mut coin = CoinConfig::olc();
    if let Some(name) = args.value("name") {
        coin.name = name.to_string();
        coin.message = format!("{name} genesis");
    }
    if let Some(ticker) = args.value("ticker") {
        coin.ticker = ticker.to_string();
    }
    coin.rules.block_time_ms = args.number("block-time-ms", coin.rules.block_time_ms)?;
    coin.rules.reward = args.number("reward", coin.rules.reward / COIN)? * COIN;
    coin.rules.halving_interval = args.number("halving", coin.rules.halving_interval)?;
    coin.initial_difficulty = args.number("difficulty", coin.initial_difficulty)?;
    let text =
        serde_json::to_string_pretty(&CoinFile::from(coin)).map_err(|error| error.to_string())?;
    std::fs::write(path, text + "\n").map_err(|error| format!("{path}: {error}"))?;
    println!("{path}");
    Ok(())
}
