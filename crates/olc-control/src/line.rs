//! Lines for a board, from a program or from a person at the serial port. A value with spaces goes
//! in double quotes: `wifi "My Home" "pass word"`. Inside quotes `\"` and `\\` stand for a quote and
//! a backslash. A quote in the middle of a word is a plain letter.
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// Bytes a board keeps of its name.
pub const MOST_NAME_BYTES: usize = 32;
/// Bytes in a Wi-Fi network's name, as the Wi-Fi standard has it.
pub const MOST_WIFI_NAME_BYTES: usize = 32;
/// Letters in a WPA2 password, as the Wi-Fi standard has it.
pub const FEWEST_WIFI_PASSWORD_BYTES: usize = 8;
pub const MOST_WIFI_PASSWORD_BYTES: usize = 63;

/// The words of a line, with quotes taken off.
pub fn split(line: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut letters = line.chars().peekable();
    while let Some(first) = letters.next() {
        if first.is_whitespace() {
            continue;
        }
        let mut word = String::new();
        if first == '"' {
            while let Some(letter) = letters.next() {
                match letter {
                    '"' => break,
                    '\\' if matches!(letters.peek(), Some('"' | '\\')) => {
                        word.extend(letters.next());
                    }
                    _ => word.push(letter),
                }
            }
        } else {
            word.push(first);
            while let Some(letter) = letters.next_if(|letter| !letter.is_whitespace()) {
                word.push(letter);
            }
        }
        words.push(word);
    }
    words
}

/// A value as one word of a line.
pub fn quoted(value: &str) -> String {
    let plain = !value.is_empty() && !value.starts_with('"') && !value.contains(char::is_whitespace);
    if plain {
        return value.to_string();
    }
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

/// A value that must not read as a word a board knows, like a network called `off`.
pub fn always_quoted(value: &str) -> String {
    match quoted(value) {
        plain if plain == value => format!("\"{plain}\""),
        marked => marked,
    }
}

/// The name in a `name` line. With no quotes the rest of the line is the name, so a person at the
/// port can type it as it reads.
pub fn name_in(line: &str) -> Option<String> {
    let rest = line.trim().strip_prefix("name")?.trim();
    let name = match rest.starts_with('"') {
        true => split(rest).into_iter().next()?,
        false => rest.to_string(),
    };
    Some(name).filter(|name| !name.is_empty())
}

/// The network and password in a `wifi` line. Without quotes each is one word, and more words than
/// that cannot be told apart.
pub fn wifi_in(line: &str) -> Option<(String, String)> {
    let mut words = split(line).into_iter();
    let _command = words.next().filter(|word| word == "wifi")?;
    let name = words.next().filter(|name| !name.is_empty())?;
    let password = words.next().unwrap_or_default();
    words.next().is_none().then_some((name, password))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_hold_spaces_and_korean() {
        assert_eq!(split("wifi \"My Home\" \"비밀 번호\""), ["wifi", "My Home", "비밀 번호"]);
        assert_eq!(split("  mine   on "), ["mine", "on"]);
        assert_eq!(split("say a\"b"), ["say", "a\"b"]);
    }

    #[test]
    fn every_value_comes_back_from_its_word() {
        for value in ["home", "내 보드 2", "a \"b\" c", "back\\slash here", "\"", "  sides  ", ""] {
            let line = format!("wifi {} x", quoted(value));
            assert_eq!(split(&line), ["wifi", value, "x"], "{value}");
        }
    }

    #[test]
    fn a_word_stays_plain_when_it_can() {
        assert_eq!(quoted("home"), "home");
        assert_eq!(always_quoted("off"), "\"off\"");
        assert_eq!(always_quoted("my off"), "\"my off\"");
    }

    #[test]
    fn a_person_can_type_a_name_with_or_without_quotes() {
        assert_eq!(name_in("name 내 보드 2").as_deref(), Some("내 보드 2"));
        assert_eq!(name_in("name \"내 보드 2\"").as_deref(), Some("내 보드 2"));
        assert_eq!(name_in("name"), None);
        assert_eq!(name_in("name \"\""), None);
    }

    #[test]
    fn wifi_takes_a_name_and_maybe_a_password() {
        let parts = |name: &str, password: &str| Some((name.to_string(), password.to_string()));
        assert_eq!(wifi_in("wifi home"), parts("home", ""));
        assert_eq!(wifi_in("wifi home secret123"), parts("home", "secret123"));
        assert_eq!(wifi_in("wifi \"My Home\" \"pass word\""), parts("My Home", "pass word"));
        assert_eq!(wifi_in("wifi My Home secret123"), None);
        assert_eq!(wifi_in("wifi"), None);
    }
}
