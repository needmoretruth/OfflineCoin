//! Where every word lives in the app. The tests read the words from the core and the firmware,
//! so a new word with no row here fails.
pub struct Row {
    pub word: &'static str,
    /// The page it is on.
    pub page: &'static str,
    /// Where on that page.
    pub spot: &'static str,
    /// Empty when anyone can reach it with no typing. Otherwise why only an expert needs it.
    pub expert_because: &'static str,
}

const fn row(word: &'static str, page: &'static str, spot: &'static str) -> Row {
    Row { word, page, spot, expert_because: "" }
}

const fn expert(word: &'static str, page: &'static str, spot: &'static str, why: &'static str) -> Row {
    Row { word, page, spot, expert_because: why }
}

const HOME: &str = "page.home";
const BOARDS: &str = "page.boards";
const COINS: &str = "page.coins";
const RECORDS: &str = "page.records";
const SETTINGS: &str = "page.settings";

/// The words of the command line.
pub const CLI_ROWS: &[Row] = &[
    row("new-coin", COINS, "New coin"),
    row("keys", SETTINGS, "Wallet, addresses"),
    row("key", SETTINGS, "Wallet, addresses"),
    row("backup", SETTINGS, "Wallet, back up"),
    row("restore", SETTINGS, "Wallet, restore"),
    row("miner", SETTINGS, "Mining, where coins go"),
    row("keep", SETTINGS, "Keeping blocks"),
    row("bluetooth", SETTINGS, "Link order, Bluetooth"),
    expert("espnow", SETTINGS, "Network, ESP-NOW card", "Needs a Wi-Fi card in monitor mode and its system name"),
    row("merge", SETTINGS, "Keeping blocks, merge"),
    row("light", SETTINGS, "Keeping blocks"),
    row("mining", SETTINGS, "Mining, start on open"),
    row("watch", SETTINGS, "Wallet, watch"),
    row("prepare", SETTINGS, "Wallet, pay from an offline computer"),
    row("sign", SETTINGS, "Wallet, pay from an offline computer"),
    row("broadcast", SETTINGS, "Wallet, pay from an offline computer"),
    row("note", RECORDS, "Write a note"),
    row("mine-note", SETTINGS, "Mining, words in each block"),
    row("snapshot", COINS, "The card that appears when old blocks are missing"),
    row("explore", RECORDS, "Search"),
    row("mine", HOME, "Start and stop"),
    row("cap", SETTINGS, "Mining, speed"),
    row("pay", HOME, "Send"),
    row("add", COINS, "Add an address"),
    row("scan", COINS, "Look again"),
    row("join", COINS, "Join from a file"),
    row("save-coin", COINS, "Save a coin file"),
    row("boards", BOARDS, "The list"),
    row("board", BOARDS, "The rows of a board"),
    row("update", BOARDS, "Update"),
    row("move", BOARDS, "Move to this coin"),
    expert("cap-test", BOARDS, "Lab", "Tuning a board's cap is measurement, and the buttons already set speed"),
    row("install", BOARDS, "New board"),
    row("pool-start", BOARDS, "Together, start"),
    row("pool-stop", BOARDS, "Together, stop"),
    expert("lab-stop", BOARDS, "Lab", "It stops an experiment, and experiments are expert work"),
    row("node-start", BOARDS, "This computer"),
    row("node-stop", BOARDS, "This computer"),
    row("node-remove", BOARDS, "This computer"),
    expert("node-cap", BOARDS, "This computer", "A node's cap is a number for tests between nodes"),
    row("node-mine", BOARDS, "This computer"),
    row("node-miner", BOARDS, "This computer, pay"),
    row("node-pay", BOARDS, "This computer, pay"),
];

/// The words a board reads.
pub const BOARD_ROWS: &[Row] = &[
    expert("status", BOARDS, "Board, read", "Raw report for diagnosis; the same numbers show on the rows"),
    expert("screen", BOARDS, "Board, read", "Raw copy of the board's screen, for diagnosis"),
    expert("boots", BOARDS, "Board, read", "Start history, for diagnosis"),
    expert("logo", BOARDS, "Board, screen", "Plays the logo once, only useful to test the screen"),
    row("view", BOARDS, "Board, screen"),
    row("lang", BOARDS, "Board, language"),
    row("mine", BOARDS, "Board, mining"),
    expert("limit", BOARDS, "Board, mining", "An exact cap; a person changes speed in Settings"),
    expert("smooth", BOARDS, "Board, screen", "Frame rate of the logo, a tuning number"),
    expert("power", BOARDS, "Board, radio", "Radio power is a tuning number"),
    row("bright", BOARDS, "Board, brightness"),
    expert("ignore", BOARDS, "Board, radio", "Blocking a neighbour is for radio tests"),
    row("offline", BOARDS, "Board, keep to itself"),
    row("espnow", BOARDS, "Board, nearby boards"),
    row("bluetooth", BOARDS, "Board, Bluetooth"),
    expert("links", BOARDS, "Board, radio", "Link order for one board; Settings sets the order for all"),
    expert("near", BOARDS, "Board, read", "A list of radio neighbours, for diagnosis"),
    expert("wires", BOARDS, "Board, radio", "Pin swap and wire speed are for wiring tests"),
    row("pay", BOARDS, "Board, pay"),
    expert("pool", BOARDS, "Board, chain", "Clearing waiting payments is a repair"),
    expert("tx", BOARDS, "Board, read", "Looking up one payment id, for diagnosis"),
    expert("trust", BOARDS, "Board, radio", "Installs a control key; it needs a key in hand"),
    row("name", BOARDS, "Board, name"),
    expert("clock", BOARDS, "Board, mining", "Chip clock is a tuning number"),
    expert("full", BOARDS, "Board, mining", "Top clock with no cap, which heats the board"),
    row("network", BOARDS, "Board, own Wi-Fi"),
    row("wifi", BOARDS, "Board, home Wi-Fi"),
    expert("dial", BOARDS, "Board, radio", "Dialing one address is a network test"),
    expert("keys", BOARDS, "Board, read", "Raw key list, for diagnosis"),
    expert("key", BOARDS, "Board, chain", "A board's keys are for experts; the app's wallet is the person's"),
    row("mineto", BOARDS, "Board, coins go to"),
    expert("keep", BOARDS, "Board, chain", "Block keeping on a board is a storage tuning"),
    expert("light", BOARDS, "Board, chain", "Headers only on a board is a storage tuning"),
    expert("merge", BOARDS, "Board, chain", "Merging on a board is a storage tuning"),
    expert("note", BOARDS, "Board, chain", "Notes go through the app in Records"),
    expert("minenote", BOARDS, "Board, chain", "Words in a board's blocks; Settings sets them for the app"),
    expert("snapshot", BOARDS, "Board, chain", "Only a board stuck without old blocks needs it"),
    row("coin", BOARDS, "Board, move to this coin"),
    expert("work", BOARDS, "Board, chain", "The pool sends it by itself, and the stop word is a repair"),
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::WORDS;
    use crate::pages::all;

    /// The first word of each arm that runs a line, such as `(Some("espnow" | "bluetooth"), ..)`.
    fn firmware_words() -> Vec<String> {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../firmware/src/");
        let mut words = Vec::new();
        for file in ["node.rs", "commands.rs", "links.rs"] {
            let text = std::fs::read_to_string(format!("{root}{file}")).unwrap();
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
        let titles: Vec<&str> = all().iter().map(|page| page.title()).collect();
        for row in CLI_ROWS.iter().chain(BOARD_ROWS) {
            assert!(titles.contains(&row.page), "{} points at {}", row.word, row.page);
            assert!(!row.spot.is_empty(), "{} has no spot", row.word);
        }
    }
}
