//! Coin files: the JSON form of a coin, with addresses written as text.
use olc_core::CoinConfig;
use olc_store::coin_file;

pub fn load(path: Option<&str>) -> Result<CoinConfig, String> {
    let Some(path) = path else {
        return Ok(CoinConfig::olc());
    };
    let text = olc_store::read_replaced(std::path::Path::new(path)).map_err(|error| format!("{path}: {error}"))?;
    coin_file::from_json(&text).map_err(|error| format!("{path}: {error}"))
}
