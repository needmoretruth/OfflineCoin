//! Numbers as people read them.
use crate::core::{format_coins, group_thousands};
use crate::i18n::{self, Voice};
use olc_core::Hash;

pub fn number(value: u128) -> String {
    group_thousands(value)
}

/// A speed in the reader's words: tries a second, or H/s for an expert.
pub fn rate(voice: Voice, value: u64) -> String {
    i18n::fill(i18n::text(voice, "unit.rate"), &[&number(value.into())])
}

/// A length of time in whole seconds, then minutes, as anyone would say it.
pub fn seconds(voice: Voice, value: u64) -> String {
    if value < 120 {
        return i18n::fill(i18n::text(voice, "unit.seconds"), &[&number(value.into())]);
    }
    i18n::fill(i18n::text(voice, "unit.minutes"), &[&number((value / 60).into())])
}

/// About how long a wait of `seconds` is, in minutes, hours or days as anyone would say it.
pub fn span(voice: Voice, seconds: u64) -> String {
    let minutes = seconds.div_ceil(60).max(1);
    let (hours, days) = (minutes / 60, minutes / 1_440);
    let (key, values) = match minutes {
        0..=59 => ("span.minutes", vec![minutes]),
        60..=1_439 if minutes.is_multiple_of(60) => ("span.hours", vec![hours]),
        60..=1_439 => ("span.hours_minutes", vec![hours, minutes % 60]),
        _ if hours.is_multiple_of(24) => ("span.days", vec![days]),
        _ => ("span.days_hours", vec![days, hours % 24]),
    };
    let values: Vec<String> = values.into_iter().map(|value| number(value.into())).collect();
    let values: Vec<&str> = values.iter().map(String::as_str).collect();
    i18n::fill(i18n::text(voice, key), &values)
}

/// How long ago a moment was.
pub fn ago(voice: Voice, now_ms: u64, then_ms: u64) -> String {
    let minutes = now_ms.saturating_sub(then_ms) / 60_000;
    let (key, count) = match minutes {
        0 => return i18n::text(voice, "time.now").to_string(),
        1..=59 => ("time.minutes", minutes),
        60..=1_439 => ("time.hours", minutes / 60),
        _ => ("time.days", minutes / 1_440),
    };
    i18n::fill(i18n::text(voice, key), &[&number(count.into())])
}

pub fn coins(units: u64, ticker: &str) -> String {
    format!("{} {ticker}", amount(units))
}

/// An amount with no coin's name beside it. No page draws an amount any other way.
pub fn amount(units: u64) -> String {
    format_coins(units)
}

pub fn short_id(hash: &Hash) -> String {
    hash[..4].iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn mebibytes(bytes: u64) -> String {
    let value = bytes as f64 / (1024.0 * 1024.0);
    if bytes > 0 && value < 1.0 {
        return format!("{} KiB", (bytes / 1024).max(1));
    }
    match value < 10.0 {
        true => format!("{value:.1} MiB"),
        false => format!("{value:.0} MiB"),
    }
}

/// A long address, with its middle left out.
pub fn short_address(address: &str) -> String {
    let count = address.chars().count();
    if count <= 20 {
        return address.to_string();
    }
    let head: String = address.chars().take(10).collect();
    let tail: String = address.chars().skip(count - 8).collect();
    format!("{head}...{tail}")
}

pub fn device(voice: Voice, kind: &str) -> String {
    let key = match kind {
        "computer" => "device.computer",
        "raspberry-pi" => "device.raspberry_pi",
        "esp32-s3" => "device.esp32",
        "" => "device.unknown",
        other => return other.to_string(),
    };
    i18n::text(voice, key).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_group_by_three() {
        assert_eq!(number(0), "0");
        assert_eq!(number(999), "999");
        assert_eq!(number(20_000), "20,000");
        assert_eq!(number(1_234_567), "1,234,567");
        assert_eq!(short_address("olc0123456789abcdef0123456789"), "olc0123456...23456789");
    }
}
