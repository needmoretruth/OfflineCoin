//! Coin amounts as people type them, and the coin files this app keeps.
use crate::settings::{Paths, Settings};
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

/// A coin this app ran, by its folder name. The built-in coin has no file of its own, so its
/// folder or its ticker names it too. The OLC of 0.1 and 0.2 is not offered: its chain ended.
pub fn known_coin(paths: &Paths, name: &str) -> Option<CoinConfig> {
    let olc = CoinConfig::olc();
    let kept = load_coin(paths, name).filter(|coin| !coin.is_retired_olc());
    kept.or_else(|| (name == coin_folder(&olc) || name.eq_ignore_ascii_case(&olc.ticker)).then_some(olc))
}

/// Moves an app that kept the OLC of 0.1 and 0.2 to today's OLC, which started over from a new
/// first block in 0.3. The wallet is the same and the old blocks stay in their folder. Returns
/// the folder it left.
pub fn leave_retired_olc(paths: &Paths, settings: &mut Settings) -> Option<String> {
    let name = settings.coin.clone()?;
    load_coin(paths, &name).filter(CoinConfig::is_retired_olc)?;
    settings.coin = Some(coin_folder(&CoinConfig::olc()));
    settings.old_olc_note = true;
    // Unsaved, the next start moves it again.
    let _ = settings.save(&paths.root);
    Some(name)
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

    /// The coin file OfflineCoin 0.2 wrote for its built-in OLC.
    const OLC_0_2: &str = r#"{"name": "OfflineCoin", "ticker": "OLC", "message": "OLCC genesis", "genesis_time_ms": 1793491200000, "initial_difficulty": 900000, "premine": [], "rules": {"block_time_ms": 30000, "retarget_interval": 10, "reward": 5000000000, "halving_interval": 210000, "coinbase_maturity": 3, "max_block_bytes": 4096, "min_fee": 0, "min_difficulty": 30000, "floor_from_height": 20000}, "message_in_block": false, "upgrade_from": 6240}"#;

    #[test]
    fn an_app_that_kept_the_olc_of_0_2_runs_todays_olc() {
        let root = std::env::temp_dir().join(format!("olc-retired-{}", std::process::id()));
        let paths = Paths { root: root.clone() };
        let old = paths.coin("82816fe40cdb");
        fs::create_dir_all(&old).unwrap();
        fs::write(old.join(COIN_FILE), OLC_0_2).unwrap();
        let mut settings = Settings { coin: Some("82816fe40cdb".into()), ..Settings::default() };

        assert!(known_coin(&paths, "82816fe40cdb").is_none(), "the old OLC is offered as a coin");
        assert_eq!(leave_retired_olc(&paths, &mut settings).as_deref(), Some("82816fe40cdb"));
        assert_eq!(settings.coin, Some(coin_folder(&CoinConfig::olc())));
        assert!(settings.old_olc_note, "Home does not say why the coins from before are gone");
        assert_eq!(Settings::load(&root).coin, settings.coin, "the move is not kept");
        assert_eq!(leave_retired_olc(&paths, &mut settings), None);
        assert!(old.join(COIN_FILE).exists(), "the old blocks' folder must stay");

        let mut other = CoinConfig::olc();
        other.message = "a coin of its own".into();
        save_coin(&paths, &other).unwrap();
        let mut on_other = Settings { coin: Some(coin_folder(&other)), ..Settings::default() };
        assert_eq!(leave_retired_olc(&paths, &mut on_other), None);
        assert!(known_coin(&paths, &coin_folder(&other)).is_some());
        let _ = fs::remove_dir_all(root);
    }
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
