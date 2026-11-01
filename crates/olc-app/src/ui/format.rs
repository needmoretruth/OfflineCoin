//! Numbers as people read them.
use crate::core::format_coins;
use crate::i18n::{self, Lang};
use olc_core::Hash;

pub fn number(value: u128) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

pub fn rate(value: u64) -> String {
    format!("{} H/s", number(value.into()))
}

pub fn coins(units: u64, ticker: &str) -> String {
    format!("{} {ticker}", format_coins(units))
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

pub fn device(lang: Lang, kind: &str) -> String {
    let key = match kind {
        "computer" => "device.computer",
        "raspberry-pi" => "device.raspberry_pi",
        "esp32-s3" => "device.esp32",
        "" => "device.unknown",
        other => return other.to_string(),
    };
    i18n::text(lang, key).to_string()
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
