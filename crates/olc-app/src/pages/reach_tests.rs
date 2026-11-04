use super::*;
use crate::core::{BoardAction, WORDS};
use crate::pages::all;

/// The files of a page.
const FILES: &[(&str, &[&str])] = &[
    (HOME, &["home.rs", "money.rs", "send.rs", "mining_card.rs", "speed.rs", "history.rs", "history_detail.rs"]),
    (MINING, &["mining.rs", "miners.rs", "speed.rs", "local.rs", "pool.rs", "pool_join.rs", "pool_parts.rs"]),
    (
        WALLET,
        &[
            "wallet.rs",
            "history.rs",
            "history_detail.rs",
            "money.rs",
            "send.rs",
            "wallet_tools.rs",
            "seed_words.rs",
            "shared_wallets.rs",
            "message_sign.rs",
            "locks.rs",
            "wallet_seal.rs",
            "wallet_contacts.rs",
        ],
    ),
    (LAB, &["lab.rs", "lab_view.rs", "lab_boards.rs"]),
    (BOARDS, &["boards.rs", "board_detail.rs", "board_more.rs"]),
    (COINS, &["coins.rs", "coins_peers.rs", "coin_new.rs", "coin_rows.rs", "coin_rule.rs", "coin_rule_data.rs", "coin_fork.rs"]),
    (RECORDS, &["records.rs"]),
    (SWAP, &["swap.rs"]),
    (SETTINGS, &["settings.rs", "settings_more.rs", "settings_compare.rs"]),
    (CHANNELS, &["channels.rs", "channels_parts.rs"]),
];
/// What only an expert ever sees, so it cannot back a row that is for anyone.
const EXPERT_FILES: &[&str] = &["board_more.rs", "lab_boards.rs", "coin_fork.rs", "locks.rs"];

fn read(path: &str) -> String {
    std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../").to_string() + path).unwrap()
}

/// What a file runs: no comments, and nothing of its tests.
fn code_of(path: &str) -> String {
    let text = read(path);
    let text = text.split("#[cfg(test)]").next().unwrap_or_default();
    let lines = text.lines().filter(|line| !line.trim_start().starts_with("//"));
    lines.collect::<Vec<_>>().join("\n")
}

fn files_of(page: &str) -> &'static [&'static str] {
    FILES.iter().find(|(name, _)| *name == page).map_or(&[], |(_, files)| files)
}

/// The first word of each arm that runs a line, such as `(Some("espnow" | "bluetooth"), ..)`.
fn firmware_words() -> Vec<String> {
    let mut words = Vec::new();
    for file in ["node.rs", "commands.rs", "links.rs"] {
        let text = read(&format!("../firmware/src/{file}"));
        for part in text.split("(Some(").skip(1) {
            let Some(end) = part.find(')') else { continue };
            let inside = &part[..end];
            if !inside.starts_with('"') || inside.contains('@') {
                continue;
            }
            words.extend(inside.split('|').map(|word| word.trim().trim_matches('"').to_string()));
        }
    }
    words.sort();
    words.dedup();
    words
}

/// Every action a board word can become, as `BoardAction::Name`, with the lines they send.
fn actions_of(word: &str) -> Vec<(String, String)> {
    let values = ["on", "off", "1", "a", "default", "clear", "stop", "basic", "en", "new", "use", "ok", "own", "accept"];
    let mut lines: Vec<Vec<&str>> = vec![vec![word]];
    for first in values {
        lines.push(vec![word, first]);
        for second in values {
            lines.push(vec![word, first, second]);
        }
    }
    let actions = lines.iter().filter_map(|words| BoardAction::parse(words));
    let named = actions.filter_map(|action| {
        let shown = format!("{action:?}");
        let name = shown.split(|letter: char| !letter.is_alphanumeric()).next()?.to_string();
        Some((format!("BoardAction::{name}"), action.line()?))
    });
    named.collect()
}

#[test]
fn every_command_line_word_has_a_place() {
    for word in WORDS {
        assert!(CLI_ROWS.iter().any(|row| row.word == word.name), "{} has no row", word.name);
    }
    for row in CLI_ROWS {
        assert!(WORDS.iter().any(|word| word.name == row.word), "{} is gone", row.word);
    }
}

#[test]
fn every_board_word_has_a_place() {
    let words = firmware_words();
    assert!(words.len() > 30, "only {} words were read", words.len());
    for word in &words {
        assert!(BOARD_ROWS.iter().any(|row| row.word == word), "{word} has no row");
    }
    for row in BOARD_ROWS {
        assert!(words.iter().any(|word| word == row.word), "{} is gone", row.word);
    }
}

#[test]
fn every_row_names_a_real_place() {
    // A place is a page of the side bar, or a tab a page turns to by that name, as swaps in Payments.
    let mut pages = all();
    for row in CLI_ROWS.iter().chain(BOARD_ROWS) {
        let real = pages.iter_mut().any(|page| page.title() == row.page || page.open_inside(row.page));
        assert!(real, "{} points at {}", row.word, row.page);
        assert!(!row.spot.is_empty(), "{} has no spot", row.word);
    }
}

#[test]
fn every_row_names_what_its_page_really_does() {
    for row in CLI_ROWS.iter().chain(BOARD_ROWS) {
        if row.sends.is_empty() {
            assert!(!row.expert_because.is_empty(), "{} has no place and no reason", row.word);
            assert_eq!(row.spot, "Terminal only", "{} claims a place it does not fill", row.word);
            continue;
        }
        let files = files_of(row.page);
        let sending = |file: &&&str| code_of(&format!("olc-app/src/pages/{file}")).contains(row.sends);
        let found: Vec<&&str> = files.iter().filter(sending).collect();
        assert!(!found.is_empty(), "{} says {} does {}", row.word, row.page, row.sends);
        if row.expert_because.is_empty() {
            let open = found.iter().any(|file| !EXPERT_FILES.contains(file));
            assert!(open, "{} is for anyone, but only an expert sees {}", row.word, row.sends);
        }
    }
}

#[test]
fn every_command_line_word_goes_where_its_page_goes() {
    let words = code_of("olc-service/src/core/words.rs");
    let sent_by_both = |row: &&Row| row.sends.starts_with("Command::") || row.sends.starts_with("Personal::");
    for row in CLI_ROWS.iter().filter(sent_by_both) {
        assert!(words.contains(row.sends), "{} does not send {}", row.word, row.sends);
    }
    // The form builds a coin with the same function that makes the word's file.
    let service = code_of("olc-service/src/core/service.rs");
    assert!(words.contains("Command::NewCoinFile") && service.contains("new_coin("));
    assert!(words.contains("LockOrder::Pay") && words.contains("LockOrder::Spend"));
}

#[test]
fn every_board_row_sends_its_own_word() {
    for row in BOARD_ROWS.iter().filter(|row| row.sends.starts_with("BoardAction::")) {
        let actions = actions_of(row.word);
        let line = actions.iter().find(|(name, _)| name == row.sends);
        let Some((_, line)) = line else {
            panic!("{} never becomes {}: {actions:?}", row.word, row.sends);
        };
        assert_eq!(line.split(' ').next(), Some(row.word), "{} sends {line}", row.sends);
    }
}

#[test]
fn a_row_cannot_name_what_no_page_does() {
    let every_file = FILES.iter().flat_map(|(_, files)| files.iter());
    let code: String = every_file.map(|file| code_of(&format!("olc-app/src/pages/{file}"))).collect();
    assert!(code.contains("BoardAction::Name"));
    assert!(!code.contains("BoardAction::Nothing"));
}
