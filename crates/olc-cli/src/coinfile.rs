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
    let olc = CoinConfig::olc();
    let coin = coin_file::new_coin(coin_file::NewCoin {
        name: args.value("name").map(str::to_string),
        message: args.value("message").map(str::to_string),
        message_in_block: true,
        ticker: args.value("ticker").map(str::to_string),
        block_time_ms: Some(args.number("block-time-ms", olc.rules.block_time_ms)?),
        reward: Some(args.number("reward", olc.rules.reward / COIN)?),
        halving: Some(args.number("halving", olc.rules.halving_interval)?),
        difficulty: Some(args.number("difficulty", NEW_COIN_DIFFICULTY)?),
        ..coin_file::NewCoin::default()
    });
    let text = coin_file::to_json(&coin)?;
    std::fs::write(path, text + "\n").map_err(|error| format!("{path}: {error}"))?;
    println!("{path}");
    Ok(())
}
