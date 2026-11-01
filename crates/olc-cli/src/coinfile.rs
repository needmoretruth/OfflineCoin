//! Coin files: the JSON form of a coin, with addresses written as text.
use olc_board::Args;
use olc_core::coin::COIN;
use olc_core::CoinConfig;
use olc_store::coin_file;

/// About what one relaxed board finds in a 30 s block. It rises by itself with more boards.
const NEW_COIN_DIFFICULTY: u64 = 30_000;

pub fn load(path: Option<&str>) -> Result<CoinConfig, String> {
    let Some(path) = path else {
        return Ok(CoinConfig::olc());
    };
    let text = std::fs::read_to_string(path).map_err(|error| format!("{path}: {error}"))?;
    coin_file::from_json(&text).map_err(|error| format!("{path}: {error}"))
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
    coin.initial_difficulty = args.number("difficulty", NEW_COIN_DIFFICULTY)?;
    let text = coin_file::to_json(&coin)?;
    std::fs::write(path, text + "\n").map_err(|error| format!("{path}: {error}"))?;
    println!("{path}");
    Ok(())
}
