//! Coin amounts as people type and read them, without passing through a float.
use crate::coin::COIN;
use alloc::format;
use alloc::string::{String, ToString};

const DECIMALS: usize = 8;

/// `1,234` as `1234`. A comma that does not group thousands is refused, because `2,5` may mean 2.5.
fn without_groups(whole: &str) -> Option<String> {
    let mut groups = whole.split(',');
    let first = groups.next()?;
    let rest_fits = groups.all(|group| group.len() == 3);
    let first_fits = !whole.contains(',') || (1..=3).contains(&first.len());
    (rest_fits && first_fits).then(|| whole.replace(',', ""))
}

/// Reads `1.5` or `1,234.5` as base units.
pub fn parse_coins(text: &str) -> Option<u64> {
    let text = text.trim();
    let (whole, part) = text.split_once('.').unwrap_or((text, ""));
    let whole = without_groups(whole)?;
    let whole = whole.as_str();
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

/// `1234567` as `1,234,567`.
pub fn group_thousands(value: u128) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

/// How an amount reads anywhere a person sees it: thousands grouped, and only the decimals it has.
pub fn format_coins(units: u64) -> String {
    let whole = group_thousands((units / COIN).into());
    let part = units % COIN;
    if part == 0 {
        return whole;
    }
    let text = format!("{whole}.{part:08}");
    text.trim_end_matches('0').to_string()
}
