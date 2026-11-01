//! All words of the app: one table per language, English first. A key missing from either fails
//! a test, and so does a key used in the code that no table has. Expert tables hold only the words
//! that differ from basic ones.
mod en;
mod en_expert;
mod ko;
mod ko_expert;
mod lab;

pub use crate::settings::{Lang, Mode};
pub use lab::lab_line;

use std::collections::HashMap;
use std::sync::OnceLock;

type Table = HashMap<&'static str, &'static str>;

fn table(lang: Lang) -> &'static Table {
    static EN: OnceLock<Table> = OnceLock::new();
    static KO: OnceLock<Table> = OnceLock::new();
    match lang {
        Lang::En => EN.get_or_init(|| en::WORDS.iter().copied().collect()),
        Lang::Ko => KO.get_or_init(|| ko::WORDS.iter().copied().collect()),
    }
}

fn expert_table(lang: Lang) -> &'static Table {
    static EN: OnceLock<Table> = OnceLock::new();
    static KO: OnceLock<Table> = OnceLock::new();
    match lang {
        Lang::En => EN.get_or_init(|| en_expert::WORDS.iter().copied().collect()),
        Lang::Ko => KO.get_or_init(|| ko_expert::WORDS.iter().copied().collect()),
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

    fn keys(words: &[(&str, &str)]) -> BTreeSet<String> {
        words.iter().map(|(key, _)| (*key).to_string()).collect()
    }

    #[test]
    fn both_languages_have_every_key_once() {
        assert_eq!(keys(en::WORDS), keys(ko::WORDS));
        assert_eq!(keys(en::WORDS).len(), en::WORDS.len());
        assert_eq!(keys(ko::WORDS).len(), ko::WORDS.len());
    }

    fn voice(lang: Lang) -> Voice {
        Voice { lang, mode: Mode::Basic }
    }

    #[test]
    fn expert_words_only_replace_basic_ones() {
        for (key, _) in en_expert::WORDS.iter().chain(ko_expert::WORDS) {
            assert_ne!(text(voice(Lang::En), key), "?", "{key}");
        }
        let expert = Voice { lang: Lang::Ko, mode: Mode::Expert };
        assert_eq!(text(expert, "notice.saved"), text(voice(Lang::Ko), "notice.saved"));
    }

    #[test]
    fn a_blank_takes_the_same_values_in_both_languages() {
        for (key, english) in en::WORDS {
            let korean = text(voice(Lang::Ko), key);
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
        sources(&folder.join("core"), &mut texts);
        let known = keys(en::WORDS);
        let prefixes: BTreeSet<&str> = en::WORDS
            .iter()
            .filter_map(|(key, _)| key.split('.').next())
            .collect();
        let mut used = 0;
        for text in texts {
            for piece in text.split('"').skip(1).step_by(2) {
                let shaped = piece.split_once('.').is_some_and(|(head, tail)| {
                    prefixes.contains(head)
                        && !tail.is_empty()
                        && tail.bytes().all(|byte| byte.is_ascii_lowercase() || byte == b'_' || byte.is_ascii_digit())
                });
                if shaped {
                    used += 1;
                    assert!(known.contains(piece), "no words for {piece}");
                }
            }
        }
        assert!(used > 100);
    }

    #[test]
    fn blanks_fill_in_order() {
        assert_eq!(fill("{} of {}", &["1", "2"]), "1 of 2");
        assert_eq!(fill("plain", &["ignored"]), "plain");
    }
}
