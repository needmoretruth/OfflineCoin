//! Coin amounts as people type them, and the coin files this app keeps.
use crate::settings::Paths;
use olc_core::coin::COIN;
use olc_core::CoinConfig;
use olc_store::coin_file;
use std::fs;
use std::path::PathBuf;

const DECIMALS: usize = 8;
const COIN_FILE: &str = "coin.json";

/// Reads `1.5` as base units, without passing through a float.
pub fn parse_coins(text: &str) -> Option<u64> {
    let text = text.trim();
    let (whole, part) = text.split_once('.').unwrap_or((text, ""));
    if part.len() > DECIMALS || (whole.is_empty() && part.is_empty()) {
        return None;
    }
    let digits = |word: &str| word.is_empty() || word.bytes().all(|byte| byte.is_ascii_digit());
    if !digits(whole) || !digits(part) {
        return None;
    }
    let whole: u64 = if whole.is_empty() { 0 } else { whole.parse().ok()? };
    let scale = 10u64.pow((DECIMALS - part.len()) as u32);
    let part: u64 = if part.is_empty() { 0 } else { part.parse().ok()? };
    whole.checked_mul(COIN)?.checked_add(part * scale)
}

pub fn format_coins(units: u64) -> String {
    let whole = units / COIN;
    let part = units % COIN;
    if part == 0 {
        return whole.to_string();
    }
    let text = format!("{whole}.{part:08}");
    text.trim_end_matches('0').to_string()
}

/// The folder name of a coin: the start of its genesis id.
pub fn coin_folder(coin: &CoinConfig) -> String {
    coin.genesis().id()[..6]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Writes the coin next to its chain and returns the file, which a node process reads.
pub fn save_coin(paths: &Paths, coin: &CoinConfig) -> Result<PathBuf, String> {
    let folder = paths.coin(&coin_folder(coin));
    fs::create_dir_all(&folder).map_err(|error| error.to_string())?;
    let file = folder.join(COIN_FILE);
    fs::write(&file, coin_file::to_json(coin)?).map_err(|error| error.to_string())?;
    Ok(file)
}

pub fn load_coin(paths: &Paths, name: &str) -> Option<CoinConfig> {
    let text = fs::read_to_string(paths.coin(name).join(COIN_FILE)).ok()?;
    coin_file::from_json(&text).ok()
}

/// `olc app` kept the default coin's blocks beside the wallet. They carry over once.
pub fn bring_old_blocks(paths: &Paths, coin: &CoinConfig) {
    let old = paths.wallet().join("blocks.log");
    let folder = paths.coin(&coin_folder(coin));
    let new = folder.join("blocks.log");
    if *coin == CoinConfig::olc() && old.exists() && !new.exists() {
        let _ = fs::create_dir_all(&folder);
        let _ = fs::copy(old, new);
    }
}

/// Reads a coin someone gave as a file.
pub fn read_coin_file(path: &str) -> Result<CoinConfig, String> {
    let text = fs::read_to_string(path).map_err(|error| format!("{path}: {error}"))?;
    coin_file::from_json(&text).map_err(|error| format!("{path}: {error}"))
}

/// Writes a coin as a file to give someone.
pub fn write_coin_file(coin: &CoinConfig, path: &str) -> Result<(), String> {
    let text = coin_file::to_json(coin)?;
    fs::write(path, text + "\n").map_err(|error| format!("{path}: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn amounts_read_exactly() {
        assert_eq!(parse_coins("1.5"), Some(150_000_000));
        assert_eq!(parse_coins("0.00000001"), Some(1));
        assert_eq!(parse_coins(".5"), Some(50_000_000));
        assert_eq!(parse_coins("2"), Some(200_000_000));
        assert_eq!(parse_coins("1.123456789"), None);
        assert_eq!(parse_coins("abc"), None);
        assert_eq!(parse_coins(""), None);
        assert_eq!(format_coins(150_000_000), "1.5");
        assert_eq!(format_coins(200_000_000), "2");
        assert_eq!(format_coins(1), "0.00000001");
    }
}
