//! Numbers as people read them.
use crate::core::{format_coins, group_thousands};
use crate::i18n::{self, Voice};
use olc_core::coin::COIN;
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

/// How long ago a moment was, in seconds for the first minute: blocks here come every few
/// seconds, so "just now" would name every row of a minute alike.
pub fn ago(voice: Voice, now_ms: u64, then_ms: u64) -> String {
    let seconds = now_ms.saturating_sub(then_ms) / 1_000;
    let minutes = seconds / 60;
    let (key, count) = match minutes {
        0 if seconds == 0 => return i18n::text(voice, "time.now").to_string(),
        0 => ("time.seconds", seconds),
        1..=59 => ("time.minutes", minutes),
        60..=1_439 => ("time.hours", minutes / 60),
        _ => ("time.days", minutes / 1_440),
    };
    i18n::fill(i18n::text(voice, key), &[&number(count.into())])
}

/// Whether a moment fell on this computer's today.
pub fn today(now_ms: u64, then_ms: u64) -> bool {
    use chrono::{Local, TimeZone};
    let day = |ms: u64| Local.timestamp_millis_opt(i64::try_from(ms).unwrap_or(i64::MAX)).single().map(|at| at.date_naive());
    day(now_ms).is_some() && day(now_ms) == day(then_ms)
}

/// How a list writes each row's moment in its column.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Told {
    /// Today's clock time and how long ago on one line, an older one's date and time.
    Line,
    /// The same on two lines, where one is too wide and rows can grow: the clock over how long
    /// ago, the date over the time.
    Stacked,
    /// How long ago alone for today's, where a column is narrow and a row holds one line.
    Short,
}

/// A moment in a list, decided for its own row, so an older row's date leaves today's rows
/// their clock time and how long ago.
pub fn listed(voice: Voice, now_ms: u64, then_ms: u64, told: Told) -> String {
    match (today(now_ms, then_ms), told) {
        (true, Told::Line) => when(voice, now_ms, then_ms),
        (true, Told::Stacked) => format!("{}\n{}", clock(now_ms, then_ms), ago(voice, now_ms, then_ms)),
        (true, Told::Short) => ago(voice, now_ms, then_ms),
        (false, Told::Stacked) => clock(now_ms, then_ms).replacen(' ', "\n", 1),
        // A narrow column keeps the date whole; the row's detail has the time.
        (false, Told::Short) => clock(now_ms, then_ms).split(' ').next().unwrap_or_default().to_string(),
        (false, Told::Line) => clock(now_ms, then_ms),
    }
}

/// The clock time of a moment on this computer: the time alone today, with the date before.
pub fn clock(now_ms: u64, then_ms: u64) -> String {
    use chrono::{Local, TimeZone};
    let local = |ms: u64| Local.timestamp_millis_opt(i64::try_from(ms).unwrap_or(i64::MAX)).single();
    match (local(now_ms), local(then_ms)) {
        (Some(now), Some(then)) if now.date_naive() == then.date_naive() => then.format("%H:%M:%S").to_string(),
        (_, Some(then)) => then.format("%Y-%m-%d %H:%M").to_string(),
        _ => String::new(),
    }
}

/// A moment as a ledger shows it: its clock time and how long ago, as `14:03:21 · 12 s ago`.
pub fn when(voice: Voice, now_ms: u64, then_ms: u64) -> String {
    format!("{} · {}", clock(now_ms, then_ms), ago(voice, now_ms, then_ms))
}

/// A moment's date and clock time on this computer, as a detail shows it.
pub fn moment(then_ms: u64) -> String {
    use chrono::{Local, TimeZone};
    let then = Local.timestamp_millis_opt(i64::try_from(then_ms).unwrap_or(i64::MAX)).single();
    then.map(|then| then.format("%Y-%m-%d %H:%M:%S").to_string()).unwrap_or_default()
}

/// A moment with its date and zone, as a file of records keeps it.
pub fn stamp(then_ms: u64) -> String {
    use chrono::{Local, TimeZone};
    let then = Local.timestamp_millis_opt(i64::try_from(then_ms).unwrap_or(i64::MAX)).single();
    then.map(|then| then.format("%Y-%m-%d %H:%M:%S %:z").to_string()).unwrap_or_default()
}

pub fn coins(units: u64, ticker: &str) -> String {
    format!("{} {ticker}", amount(units))
}

/// What a pool paid, to two decimals at most for a reader who wants to know about how much, and
/// to the last unit for an expert. A payout too small to show says so rather than reading as 0.
pub fn payout(voice: Voice, units: u64, ticker: &str) -> String {
    format!("{} {ticker}", payout_amount(voice, units))
}

/// A payout as `payout` says it, with no coin's name beside it.
pub fn payout_amount(voice: Voice, units: u64) -> String {
    if voice.mode == i18n::Mode::Expert {
        return amount(units);
    }
    let cent = COIN / 100;
    let near = units.saturating_add(cent / 2) / cent * cent;
    if units > 0 && near == 0 {
        return format!("< {}", amount(cent));
    }
    amount(near)
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

    /// The owner asked for a pool's payouts to at most two decimals in basic mode.
    #[test]
    fn a_payout_reads_to_two_decimals_and_in_full_for_an_expert() {
        let basic = Voice::default();
        let expert = Voice { mode: crate::i18n::Mode::Expert, ..basic };
        assert_eq!(payout(basic, 22_138_709_561, "OLC"), "221.39 OLC");
        assert_eq!(payout(expert, 22_138_709_561, "OLC"), "221.38709561 OLC");
        assert_eq!(payout(basic, 123_456_000_000, "OLC"), "1,234.56 OLC");
        assert_eq!(payout(basic, 5_000_000_000, "OLC"), "50 OLC");
        assert_eq!(payout(basic, 99_600_000, "OLC"), "1 OLC");
        assert_eq!(payout(basic, 10_000, "OLC"), "< 0.01 OLC");
        assert_eq!(payout(basic, 0, "OLC"), "0 OLC");
    }

    /// The owner saw "Just now" on every row while blocks came every few seconds.
    #[test]
    fn a_moment_within_the_minute_counts_its_seconds() {
        let voice = Voice::default();
        assert_eq!(ago(voice, 60_000, 48_000), "12 s ago");
        assert_ne!(ago(voice, 60_000, 55_000), ago(voice, 60_000, 50_000));
        assert_eq!(ago(voice, 200_000, 80_000), "2 min ago");
        let ko = Voice { lang: crate::i18n::Lang::Ko, mode: crate::i18n::Mode::Basic };
        assert_eq!(ago(ko, 60_000, 48_000), "12초 전");
        let shown = when(voice, 1_800_000_000_000, 1_799_999_990_000);
        assert!(shown.ends_with(" · 10 s ago") && shown.len() == "00:00:00 · 10 s ago".len(), "{shown}");
    }

    /// Each row of a list says its own time: an older row's date leaves today's rows as they are.
    #[test]
    fn a_listed_moment_is_decided_for_its_own_row() {
        use chrono::{Local, TimeZone};
        let at = |day, hour| Local.with_ymd_and_hms(2026, 11, day, hour, 0, 0).single().map_or(0, |at| at.timestamp_millis().unsigned_abs());
        let (now, earlier, yesterday) = (at(4, 12), at(4, 12) - 3_000, at(3, 8));
        let voice = Voice::default();
        assert_eq!(listed(voice, now, earlier, Told::Line), "11:59:57 · 3 s ago");
        assert_eq!(listed(voice, now, earlier, Told::Stacked), "11:59:57\n3 s ago");
        assert_eq!(listed(voice, now, earlier, Told::Short), "3 s ago");
        assert_eq!(listed(voice, now, yesterday, Told::Line), "2026-11-03 08:00");
        assert_eq!(listed(voice, now, yesterday, Told::Stacked), "2026-11-03\n08:00");
        // Home's narrow column keeps an older row's date whole; its detail has the time.
        assert_eq!(listed(voice, now, yesterday, Told::Short), "2026-11-03");
    }
}
