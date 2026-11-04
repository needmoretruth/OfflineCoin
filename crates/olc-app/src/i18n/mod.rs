//! All words of the app: one table per language, English first. A key missing from either fails
//! a test, and so does a key used in the code that no table has. Expert tables hold the words that
//! differ from basic ones, and `pro.` rows that only expert mode shows. Basic tables may not carry
//! an expert term: `plain_words_have_no_expert_terms` lists the banned ones.
mod en;
mod en_chain;
mod en_coin;
mod en_channels;
mod en_channels_expert;
mod en_expert;
mod en_expert_more;
mod en_lab2;
mod en_lab2_expert;
mod en_mining;
mod en_more;
mod en_place;
mod en_settings;
mod en_pool;
mod en_pool_expert;
mod en_seal;
mod en_wallet;
mod ko;
mod ko_chain;
mod ko_coin;
mod ko_channels;
mod ko_channels_expert;
mod ko_expert;
mod ko_expert_more;
mod ko_lab2;
mod ko_lab2_expert;
mod ko_mining;
mod ko_more;
mod ko_place;
mod ko_settings;
mod ko_pool;
mod ko_pool_expert;
mod ko_seal;
mod ko_wallet;
mod lab;

pub use crate::settings::{Lang, Mode};
pub use lab::lab_line;

use std::collections::HashMap;
use std::sync::OnceLock;

type Table = HashMap<&'static str, &'static str>;
type Words = &'static [(&'static str, &'static str)];

// Every table of each kind, in one place: a new file of words is one line in two of these lists.
const BASE_EN: &[Words] = &[
    en::WORDS,
    en_more::WORDS,
    en_chain::WORDS,
    en_lab2::WORDS,
    en_channels::WORDS,
    en_wallet::WORDS,
    en_mining::WORDS,
    en_place::WORDS,
    en_settings::WORDS,
    en_coin::WORDS,
    en_pool::WORDS,
    en_seal::WORDS,
];
const BASE_KO: &[Words] = &[
    ko::WORDS,
    ko_more::WORDS,
    ko_chain::WORDS,
    ko_lab2::WORDS,
    ko_channels::WORDS,
    ko_wallet::WORDS,
    ko_mining::WORDS,
    ko_place::WORDS,
    ko_settings::WORDS,
    ko_coin::WORDS,
    ko_pool::WORDS,
    ko_seal::WORDS,
];
const EXPERT_EN: &[Words] = &[
    en_expert::WORDS,
    en_expert_more::WORDS,
    en_chain::EXPERT,
    en_lab2_expert::WORDS,
    en_channels_expert::WORDS,
    en_wallet::EXPERT,
    en_mining::EXPERT,
    en_place::EXPERT,
    en_settings::EXPERT,
    en_coin::EXPERT,
    en_pool_expert::WORDS,
    en_seal::EXPERT,
];
const EXPERT_KO: &[Words] = &[
    ko_expert::WORDS,
    ko_expert_more::WORDS,
    ko_chain::EXPERT,
    ko_lab2_expert::WORDS,
    ko_channels_expert::WORDS,
    ko_wallet::EXPERT,
    ko_mining::EXPERT,
    ko_place::EXPERT,
    ko_settings::EXPERT,
    ko_coin::EXPERT,
    ko_pool_expert::WORDS,
    ko_seal::EXPERT,
];

fn joined(tables: &[Words]) -> Table {
    tables.iter().flat_map(|table| table.iter()).copied().collect()
}

fn table(lang: Lang) -> &'static Table {
    static EN: OnceLock<Table> = OnceLock::new();
    static KO: OnceLock<Table> = OnceLock::new();
    match lang {
        Lang::En => EN.get_or_init(|| joined(BASE_EN)),
        Lang::Ko => KO.get_or_init(|| joined(BASE_KO)),
    }
}

fn expert_table(lang: Lang) -> &'static Table {
    static EN: OnceLock<Table> = OnceLock::new();
    static KO: OnceLock<Table> = OnceLock::new();
    match lang {
        Lang::En => EN.get_or_init(|| joined(EXPERT_EN)),
        Lang::Ko => KO.get_or_init(|| joined(EXPERT_KO)),
    }
}

/// Who the words are for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Voice {
    pub lang: Lang,
    pub mode: Mode,
}

pub fn text(voice: Voice, key: &str) -> &'static str {
    let expert = match voice.mode {
        Mode::Basic => None,
        Mode::Expert => expert_table(voice.lang).get(key).copied(),
    };
    expert.or_else(|| table(voice.lang).get(key).copied()).unwrap_or("?")
}

/// Puts `values` into the `{}` of `text`, in order.
pub fn fill(text: &str, values: &[&str]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut values = values.iter();
    let mut parts = text.split("{}");
    if let Some(first) = parts.next() {
        out.push_str(first);
    }
    for part in parts {
        out.push_str(values.next().copied().unwrap_or(""));
        out.push_str(part);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::fs;
    use std::path::Path;

    type Rows = Vec<(&'static str, &'static str)>;

    fn rows(tables: &[&'static [(&'static str, &'static str)]]) -> Rows {
        tables.iter().flat_map(|table| table.iter()).copied().collect()
    }

    fn keys(words: &[(&str, &str)]) -> BTreeSet<String> {
        words.iter().map(|(key, _)| (*key).to_string()).collect()
    }

    #[test]
    fn both_languages_have_every_key_once() {
        let (english, korean) = (rows(BASE_EN), rows(BASE_KO));
        assert_eq!(keys(&english), keys(&korean));
        assert_eq!(keys(&english).len(), english.len());
        assert_eq!(keys(&korean).len(), korean.len());
        let (english, korean) = (
            rows(EXPERT_EN),
            rows(EXPERT_KO),
        );
        assert_eq!(keys(&english), keys(&korean));
        assert_eq!(keys(&english).len(), english.len());
    }

    fn voice(lang: Lang) -> Voice {
        Voice { lang, mode: Mode::Basic }
    }

    /// An expert row either replaces a basic word or is one only expert mode shows.
    #[test]
    fn expert_words_replace_basic_ones_or_are_expert_rows() {
        for (key, _) in rows(EXPERT_EN) {
            let first = key.split('.').next();
            let only_expert = matches!(first, Some("pro" | "lab" | "captest" | "console" | "lock"))
                || matches!(key, "page.console" | "page.locks" | "notice.experiment_busy");
            assert!(only_expert || text(voice(Lang::En), key) != "?", "{key}");
        }
        let expert = Voice { lang: Lang::Ko, mode: Mode::Expert };
        assert_eq!(text(expert, "notice.saved"), text(voice(Lang::Ko), "notice.saved"));
    }

    /// A plain sentence may not lean on a word from the trade. Korean is checked as written, and
    /// English as whole lowercase words so that `work` does not trip on `network`.
    const BANNED_ENGLISH: &[&str] = &[
        "hash", "hashrate", "h/s", "difficulty", "utxo", "utxos", "mempool", "node", "nodes", "peer",
        "peers", "pruning", "pruned", "prune", "esp-now", "espnow", "genesis", "ticker", "premine",
        "header", "headers", "light node", "full node", "coinbase", "txid", "merkle", "nonce",
        "broadcast", "op_return", "firmware", "ota", "uart", "snapshot", "fork", "chain", "cli",
        "command", "soft ap", "halving",
    ];
    const BANNED_KOREAN: &[&str] = &[
        "해시", "난이도", "멤풀", "노드", "피어", "프루닝", "제네시스", "티커", "프리마인", "헤더",
        "라이트", "코인베이스", "머클", "넌스", "브로드캐스트", "펌웨어", "UTXO", "ESP-NOW", "체인",
        "스냅샷", "포크", "명령어", "반감기", "전파",
    ];

    /// The developer's note is one person talking, in that person's words.
    const OWN_VOICE: &str = "note.body";

    #[test]
    fn plain_words_have_no_expert_terms() {
        for (key, words) in rows(BASE_EN) {
            if key == OWN_VOICE {
                continue;
            }
            let lower = format!(" {} ", words.to_lowercase());
            let spaced: String =
                lower.chars().map(|c| if c.is_alphanumeric() || "/_-".contains(c) { c } else { ' ' }).collect();
            for term in BANNED_ENGLISH {
                assert!(!spaced.contains(&format!(" {term} ")), "{key}: {words} says {term}");
            }
        }
        for (key, words) in rows(BASE_KO) {
            if key == OWN_VOICE {
                continue;
            }
            for term in BANNED_KOREAN {
                assert!(!words.contains(term), "{key}: {words} says {term}");
            }
        }
    }

    /// Expert mode swaps the plain terms for the standard ones where it matters most.
    #[test]
    fn expert_mode_uses_the_terms_of_the_trade() {
        let expert = Voice { lang: Lang::En, mode: Mode::Expert };
        assert_eq!(text(expert, "local.keep_all"), "Full node");
        assert_eq!(text(expert, "unit.rate"), "{} H/s");
        assert_eq!(text(expert, "reach.radio"), "ESP-NOW");
        assert_eq!(text(expert, "pro.difficulty"), "Difficulty");
        assert_eq!(text(expert, "pro.mempool"), "Mempool");
        assert_eq!(text(voice(Lang::En), "pro.mempool"), "?");
    }

    #[test]
    fn a_blank_takes_the_same_values_in_both_languages() {
        for (key, english) in rows(BASE_EN) {
            let korean = text(voice(Lang::Ko), key);
            assert_eq!(english.matches("{}").count(), korean.matches("{}").count(), "{key}");
        }
        for (key, english) in rows(EXPERT_EN) {
            let korean = text(Voice { lang: Lang::Ko, mode: Mode::Expert }, key);
            assert_eq!(english.matches("{}").count(), korean.matches("{}").count(), "{key}");
        }
    }

    fn sources(folder: &Path, found: &mut Vec<String>) {
        for entry in fs::read_dir(folder).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                sources(&path, found);
            } else if path.extension().is_some_and(|word| word == "rs") {
                found.push(fs::read_to_string(path).unwrap());
            }
        }
    }

    /// Everything shaped like `"page.network"` in the code must be a key.
    #[test]
    fn every_key_used_in_the_code_exists() {
        let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut texts = Vec::new();
        sources(&folder.join("pages"), &mut texts);
        sources(&folder.join("ui"), &mut texts);
        sources(&folder.join("../../olc-service/src/core"), &mut texts);
        sources(&folder.join("../../olc-service/src/local"), &mut texts);
        let base = rows(BASE_EN);
        let expert = rows(EXPERT_EN);
        let mut known = keys(&base);
        known.extend(keys(&expert));
        let prefixes: BTreeSet<&str> =
            known.iter().filter_map(|key| key.split('.').next()).collect();
        let mut used = 0;
        let mut seen = BTreeSet::new();
        for text in texts {
            for piece in text.split('"').skip(1).step_by(2) {
                let file = piece == "coin.json" || piece.ends_with(".rs");
                let shaped = !file && piece.split_once('.').is_some_and(|(head, tail)| {
                    prefixes.contains(head)
                        && !tail.is_empty()
                        && tail.bytes().all(|byte| byte.is_ascii_lowercase() || byte == b'_' || byte.is_ascii_digit())
                });
                if shaped {
                    used += 1;
                    assert!(known.contains(piece), "no words for {piece}");
                    seen.insert(piece.to_string());
                }
            }
        }
        assert!(used > 100);
        let unused: Vec<&String> = known.difference(&seen).collect();
        assert!(unused.is_empty(), "words no screen uses: {unused:?}");
    }

    #[test]
    fn every_reason_a_node_refuses_a_payment_reads_in_english_and_korean() {
        use crate::core::refusals::{refusal_key, ALL};
        let hangul = |c: char| ('\u{AC00}'..='\u{D7A3}').contains(&c);
        for why in ALL {
            let key = refusal_key(why);
            let (english, korean) = (text(voice(Lang::En), key), text(voice(Lang::Ko), key));
            assert!(english != "?" && !english.chars().any(hangul), "{why:?} has no English line: {english}");
            assert!(korean != "?" && korean.chars().any(hangul), "{why:?} has no Korean line: {korean}");
        }
    }

    #[test]
    fn blanks_fill_in_order() {
        assert_eq!(fill("{} of {}", &["1", "2"]), "1 of 2");
        assert_eq!(fill("plain", &["ignored"]), "plain");
    }
}
