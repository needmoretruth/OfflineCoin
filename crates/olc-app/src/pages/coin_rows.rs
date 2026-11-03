//! Rows for coin settings. A new coin and a rule change draw the same ones from the same table.
use super::Cx;
use crate::core::{Bad, CoinField, FieldKind, Why};
use crate::ui::register::row;
use super::coin_rule::rule_name;
use crate::ui::widgets::{field, meta, segments, switch};
use egui::Ui;
use olc_core::CoinConfig;

const RULE_FIELD: f32 = 200.0;
const MS_PER_SECOND: u64 = 1_000;
const BLOCK_TIME: &str = "rules.block_time_ms";

/// The words of a coin field: its name and what it means.
pub fn words_of(key: &str) -> (&'static str, &'static str) {
    match key {
        "name" => ("coin.name", "coin.name_about"),
        "ticker" => ("pro.ticker", "pro.ticker_about"),
        "message" => ("pro.message", "pro.message_about"),
        "message_in_block" => ("pro.message_in_block", "pro.message_in_block_about"),
        "genesis_time_ms" => ("pro.genesis_time", "pro.genesis_time_about"),
        "initial_difficulty" => ("pro.difficulty", "pro.difficulty_about"),
        "premine" => ("pro.premine", "pro.premine_about"),
        "rules.block_time_ms" => ("pro.block_ms", "pro.block_ms_about"),
        "rules.retarget_interval" => ("pro.retarget", "pro.retarget_about"),
        "rules.reward" => ("coin.reward", "coin.reward_about"),
        "rules.halving_interval" => ("pro.halving", "pro.halving_about"),
        "rules.coinbase_maturity" => ("pro.maturity", "pro.maturity_about"),
        "rules.max_block_bytes" => ("pro.max_block_bytes", "pro.max_block_bytes_about"),
        "rules.min_fee" => ("pro.min_fee", "pro.min_fee_about"),
        "rules.min_difficulty" => ("pro.floor", "pro.floor_about"),
        "rules.floor_from_height" => ("pro.floor_from_height", "pro.floor_from_height_about"),
        "rules.difficulty_rule" => ("coin.rule", "coin.rule_about"),
        "rules.half_life_blocks" => ("pro.half_life", "pro.half_life_about"),
        "rules.median_time_blocks" => ("pro.median_time", "pro.median_time_about"),
        "rules.max_ahead_ms" => ("pro.max_ahead", "pro.max_ahead_about"),
        "bitcoin_header" => ("pro.bitcoin_header", "pro.bitcoin_header_about"),
        _ => ("pro.fork_from", "pro.fork_from_about"),
    }
}

/// What a coin holds for a field, as the box would show it.
pub fn text_of(field: &CoinField, coin: &CoinConfig) -> String {
    (field.get)(coin)
}

/// One setting as a register row. Returns true when it changed.
pub fn rule_row(ui: &mut Ui, cx: &Cx, setting: &CoinField, text: &mut String, hint: &str) -> bool {
    let (name, about) = words_of(setting.key);
    let mut changed = false;
    row(ui, cx.t, cx.say(name), cx.say(about), |ui| match setting.kind {
        FieldKind::Switch => {
            // Empty means the coin's own value, which is on for a new coin.
            let mut on = text.as_str() != "off" && hint != "off";
            if switch(ui, cx.t, &mut on, cx.say(name)).changed() {
                *text = if on { "on" } else { "off" }.to_string();
                changed = true;
            }
        }
        FieldKind::Choice(names) => {
            // The difficulty rule is the one choice a coin has; the picker names it as the form does,
            // with the running coin's blocks between adjustments.
            let window = cx.snap.node.coin.as_ref().map_or(0, |coin| coin.rules.retarget_interval);
            let items: Vec<(&str, String)> = names.iter().map(|name| (*name, rule_name(cx, name, window))).collect();
            let now = if text.trim().is_empty() { hint } else { text.trim() };
            let current = names.iter().copied().find(|name| *name == now);
            if let Some(chosen) = segments(ui, cx.t, &items, current) {
                *text = chosen.to_string();
                changed = true;
            }
        }
        _ => {
            let width = ui.available_width().min(RULE_FIELD);
            changed = field(ui, text, hint, width).accessible_name(cx.say(name)).changed();
        }
    });
    changed
}

/// Seconds with up to three decimals, as whole milliseconds.
pub fn seconds_to_ms(text: &str) -> Option<u64> {
    let text = text.trim();
    let (whole, part) = text.split_once('.').unwrap_or((text, ""));
    if part.len() > 3 || (whole.is_empty() && part.is_empty()) {
        return None;
    }
    let digits = |word: &str| word.bytes().all(|byte| byte.is_ascii_digit());
    if !digits(whole) || !digits(part) {
        return None;
    }
    let part: u64 = format!("{part:0<3}").parse().ok()?;
    let whole: u64 = if whole.is_empty() { 0 } else { whole.parse().ok()? };
    whole.checked_mul(MS_PER_SECOND)?.checked_add(part)
}

pub fn ms_to_seconds(ms: u64) -> String {
    let text = format!("{}.{:03}", ms / MS_PER_SECOND, ms % MS_PER_SECOND);
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// What is wrong with a value, in a sentence for a person. The field goes by its name on the form.
pub fn bad_text(cx: &Cx, bad: &Bad) -> String {
    let basic_block = bad.key == BLOCK_TIME && !cx.expert();
    let name = match basic_block {
        true => cx.say("coin.block"),
        false => cx.say(words_of(&bad.key).0),
    };
    match &bad.why {
        Why::Empty => cx.fill("bad.empty", &[name]),
        Why::NotNumber => cx.fill("bad.not_number", &[name]),
        Why::NotAmount => cx.fill("bad.not_amount", &[name]),
        Why::NotSwitch => cx.fill("bad.not_switch", &[name]),
        Why::NotOneOf(names) => cx.fill("bad.not_one_of", &[name, &names.join(", ")]),
        Why::AtLeast(least) if basic_block => cx.fill("bad.at_least", &[name, &ms_to_seconds(*least)]),
        Why::AtLeast(least) => cx.fill("bad.at_least", &[name, &least.to_string()]),
        Why::AtMost(most) => cx.fill("bad.at_most", &[name, &most.to_string()]),
        Why::BadPremine(line) => cx.fill("bad.premine", &[line]),
        Why::TooLong => cx.say("bad.too_long").to_string(),
        Why::NoChange | Why::NotARule => cx.say("bad.no_change").to_string(),
    }
}

/// The line under a form that says why it cannot go on yet.
pub fn bad_line(ui: &mut Ui, cx: &Cx, bad: Option<&Bad>) {
    if let Some(bad) = bad {
        ui.add_space(8.0);
        meta(ui, &bad_text(cx, bad), cx.t);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::COIN_FIELDS;

    #[test]
    fn seconds_read_from_a_fifth_to_a_day() {
        assert_eq!(seconds_to_ms("0.2"), Some(200));
        assert_eq!(seconds_to_ms(".5"), Some(500));
        assert_eq!(seconds_to_ms("86400"), Some(86_400_000));
        assert_eq!(seconds_to_ms("1.2345"), None);
        assert_eq!(seconds_to_ms("soon"), None);
        assert_eq!(seconds_to_ms(""), None);
        assert_eq!(ms_to_seconds(200), "0.2");
        assert_eq!(ms_to_seconds(86_400_000), "86400");
    }

    #[test]
    fn every_coin_field_has_its_own_words() {
        let mut seen = std::collections::HashSet::new();
        for field in COIN_FIELDS {
            let (name, about) = words_of(field.key);
            assert_ne!(name, "pro.fork_from", "{} has no words", field.key);
            assert!(seen.insert(name), "{} shares its name", field.key);
            assert_ne!(name, about);
        }
    }
}
