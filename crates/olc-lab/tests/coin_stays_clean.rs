//! The coin must not know this crate exists. Every crate here except this one and the program
//! that registers it is the coin or generic tooling, and so is the firmware.
use std::fs;
use std::path::{Path, PathBuf};

const OUTSIDE: [&str; 4] = ["olc-lab", "olc-cli", "olc-pool", "olc-app"];
const OWN_NAMES: [&str; 2] = ["olc-lab", "olc-pool"];
/// Pool as a kind of mining. A waiting pool of payments is another thing and stays allowed.
const POOL_WORDS: [&str; 5] = [
    "olc pool",
    "mining pool",
    "pool mining",
    "share difficulty",
    "payout",
];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn coin_crates() -> Vec<PathBuf> {
    let mut crates: Vec<PathBuf> = fs::read_dir(root().join("crates"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| !OUTSIDE.iter().any(|name| path.ends_with(name)))
        .collect();
    assert!(crates.len() >= 9, "the crate folder moved");
    crates.push(root().join("firmware"));
    crates
}

fn files_under(folder: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(folder) else {
        return;
    };
    for entry in entries {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files_under(&path, found);
        } else {
            found.push(path);
        }
    }
}

fn sources() -> Vec<PathBuf> {
    let mut found = Vec::new();
    for folder in coin_crates() {
        files_under(&folder.join("src"), &mut found);
        files_under(&folder.join("tests"), &mut found);
    }
    assert!(found.len() >= 20, "the sources moved");
    found
}

/// A word in the sense of the banned list: the digits may not touch other digits or letters.
fn has_number(text: &str, number: &str) -> bool {
    text.match_indices(number).any(|(at, _)| {
        let before = text[..at].chars().next_back();
        let after = text[at + number.len()..].chars().next();
        !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric)
    })
}

fn banned(text: &str) -> Option<&'static str> {
    let flat = text.to_lowercase().replace(['_', '-'], " ");
    if flat.contains("attack") {
        return Some("attack");
    }
    if flat.contains("double spend") || flat.contains("doublespend") {
        return Some("double spend");
    }
    if let Some(word) = POOL_WORDS.iter().find(|word| flat.contains(**word)) {
        return Some(word);
    }
    has_number(&flat, "51").then_some("51")
}

#[test]
fn no_coin_crate_depends_on_the_lab() {
    for folder in coin_crates() {
        let manifest = fs::read_to_string(folder.join("Cargo.toml")).unwrap();
        for name in OWN_NAMES {
            assert!(
                !manifest.contains(name),
                "{} depends on {name}",
                folder.display()
            );
        }
    }
}

#[test]
fn no_coin_source_speaks_of_it() {
    let mut found = Vec::new();
    for path in sources() {
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        for (index, line) in text.lines().enumerate() {
            if let Some(word) = banned(line) {
                found.push(format!("{}:{} says {word}", path.display(), index + 1));
            }
        }
    }
    assert!(found.is_empty(), "{}", found.join("\n"));
}

#[test]
fn the_check_catches_what_it_looks_for() {
    assert_eq!(banned("fn attacker()"), Some("attack"));
    assert_eq!(banned("Double_Spend"), Some("double spend"));
    assert_eq!(banned("at 51 percent"), Some("51"));
    assert_eq!(banned("51%"), Some("51"));
    assert_eq!(banned("0x51ab 1510"), None);
    assert_eq!(banned("use olc_pool::Pool;"), Some("olc pool"));
    assert_eq!(banned("the payment pool is full"), None);
}
