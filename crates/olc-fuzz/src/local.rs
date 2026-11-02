//! Files a stranger hands a person, and the short words people and boards type.
use crate::fixture::wallet;
use crate::{Parse, Target};
use olc_core::CoinConfig;
use olc_node::{Medium, Rank};
use olc_store::{back_up, coin_file, parse_secret, restore};
use std::fs;
use std::path::PathBuf;

fn coin_files() -> Vec<Vec<u8>> {
    let text = coin_file::to_json(&CoinConfig::olc()).unwrap_or_default();
    vec![text.into_bytes()]
}

fn read_coin_file(bytes: &[u8]) {
    let text = String::from_utf8_lossy(bytes);
    if let Ok(coin) = coin_file::from_json(&text) {
        crate::taken();
        std::hint::black_box(coin.genesis().id());
    }
}

fn scratch(name: &str) -> PathBuf {
    let folder = std::env::temp_dir().join(format!("olc-fuzz-{}", std::process::id()));
    let _ = fs::create_dir_all(&folder);
    folder.join(name)
}

fn backups() -> Vec<Vec<u8>> {
    let file = scratch("seed.backup");
    let plain = back_up(&wallet(5), &file, None).and_then(|()| {
        fs::read(&file).map_err(|error| error.to_string())
    });
    let sealed = back_up(&wallet(5), &file, Some("a long password")).and_then(|()| {
        fs::read(&file).map_err(|error| error.to_string())
    });
    let _ = fs::remove_file(&file);
    plain.into_iter().chain(sealed).collect()
}

/// With no password, so that a sealed file stops before the slow hash. All the reading is ahead
/// of it, and the hash takes the salt as bytes of any length.
fn start_restore() -> Parse {
    let file = scratch(&format!("{:?}.backup", std::thread::current().id()));
    Box::new(move |bytes| {
        if fs::write(&file, bytes).is_ok() && restore(&file, None).is_ok() {
            crate::taken();
        }
    })
}

fn read_words(bytes: &[u8]) {
    let text = String::from_utf8_lossy(bytes);
    let rank = Rank::parse(&text).is_some();
    if rank || Medium::from_name(&text).is_some() || parse_secret(&text).is_some() {
        crate::taken();
    }
}

fn words() -> Vec<Vec<u8>> {
    let lines = ["wifi,espnow,bluetooth,wire", "wire", &"ab".repeat(32)];
    lines.iter().map(|line| line.as_bytes().to_vec()).collect()
}

pub fn targets() -> Vec<Target> {
    vec![
        Target { name: "file.coin", seeds: coin_files, start: || Box::new(read_coin_file) },
        Target { name: "file.backup", seeds: backups, start: start_restore },
        Target { name: "text.words", seeds: words, start: || Box::new(read_words) },
    ]
}
