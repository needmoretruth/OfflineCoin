//! Coin amounts as people type them, and the coin files this app keeps.
use crate::settings::Paths;
use olc_core::CoinConfig;
use olc_store::{coin_file, replace_file};
use std::fs;
use std::path::{Path, PathBuf};

pub use olc_store::amount::{format_coins, group_thousands, parse_coins};

const COIN_FILE: &str = "coin.json";

/// The folder name of a coin: the start of its genesis id, then its rule changes. A fork keeps a
/// chain of its own, so the coin it left stays whole and the way back stays open.
pub fn coin_folder(coin: &CoinConfig) -> String {
    let start: String = coin.genesis().id()[..6].iter().map(|byte| format!("{byte:02x}")).collect();
    match coin.forks_id() {
        forks if forks.is_empty() => start,
        forks => format!("{start}-{forks}"),
    }
}

/// Gives a fork's folder the chain it leaves from, with this node's settings. What a folder holds
/// already stays, so a fork a person returns to keeps the blocks it made.
pub fn copy_chain(from: &Path, to: &Path) -> Result<(), String> {
    let failed = |error: std::io::Error| error.to_string();
    fs::create_dir_all(to).map_err(failed)?;
    let Ok(files) = fs::read_dir(from) else {
        return Ok(());
    };
    for file in files.flatten() {
        let name = file.file_name();
        let target = to.join(&name);
        let held = name.to_string_lossy().ends_with(".hold");
        if file.path().is_file() && name != COIN_FILE && !held && !target.exists() {
            fs::copy(file.path(), target).map_err(failed)?;
        }
    }
    Ok(())
}

/// Writes the coin next to its chain and returns the file, which a node process reads.
pub fn save_coin(paths: &Paths, coin: &CoinConfig) -> Result<PathBuf, String> {
    let folder = paths.coin(&coin_folder(coin));
    fs::create_dir_all(&folder).map_err(|error| error.to_string())?;
    let file = folder.join(COIN_FILE);
    // Each node's start lands here while the node before it may still be reading this file.
    replace_file(&file, coin_file::to_json(coin)?.as_bytes())?;
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
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn a_starting_node_never_reads_a_coin_file_half_written() {
        let root = std::env::temp_dir().join(format!("olc-coin-file-{}", std::process::id()));
        let paths = Paths { root: root.clone() };
        let mut coin = CoinConfig::olc();
        let file = save_coin(&paths, &coin).unwrap();
        let done = Arc::new(AtomicBool::new(false));
        let reader = {
            let (file, done) = (file.clone(), done.clone());
            thread::spawn(move || {
                let mut cut = 0;
                while !done.load(Ordering::Relaxed) {
                    let text = fs::read_to_string(&file).unwrap_or_default();
                    cut += usize::from(coin_file::from_json(&text).is_err());
                }
                cut
            })
        };
        for turn in 0..3_000 {
            coin.message = format!("{turn}");
            replace_file(&file, coin_file::to_json(&coin).unwrap().as_bytes()).unwrap();
        }
        done.store(true, Ordering::Relaxed);
        let cut = reader.join().unwrap();
        let _ = fs::remove_dir_all(&root);
        assert_eq!(cut, 0, "a reader found a cut file");
    }

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

    #[test]
    fn amounts_group_their_thousands_and_read_back() {
        assert_eq!(format_coins(240_000_000_000), "2,400");
        assert_eq!(format_coins(123_456_789_050_000_000), "1,234,567,890.5");
        assert_eq!(format_coins(99_900_000_000), "999");
        for units in [0, 1, 150_000_000, 240_000_000_000, 123_456_789_050_000_000] {
            assert_eq!(parse_coins(&format_coins(units)), Some(units));
        }
    }

    #[test]
    fn a_comma_counts_only_between_thousands() {
        assert_eq!(parse_coins("2,400"), Some(240_000_000_000));
        assert_eq!(parse_coins("2,5"), None);
        assert_eq!(parse_coins("2,4000"), None);
        assert_eq!(parse_coins("1234,567"), None);
        assert_eq!(parse_coins(",400"), None);
        assert_eq!(parse_coins("1.5,000"), None);
    }
}
