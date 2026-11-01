//! The form for a new coin. The first run and the Coins place share it. A person can press go
//! with what it starts with, and an expert has every number.
use super::Cx;
use crate::core::{new_coin, parse_coins, unix_ms, NewCoin};
use crate::ui::register::row;
use crate::ui::widgets::{field, number_field, segments};
use egui::Ui;
use olc_core::coin::{COIN, MIN_DIFFICULTY};
use olc_core::{CoinConfig, Rules};
use olc_wallet::parse_address;

const BLOCK_CHOICES: [u64; 3] = [10, 30, 60];
const REWARD_CHOICES: [u64; 3] = [1, 10, 50];
const TICKER_LETTERS: usize = 3;
const NO_LETTERS: &str = "NEW";

pub struct CoinForm {
    name: String,
    ticker: String,
    block_seconds: u64,
    reward: u64,
    halving: u64,
    difficulty: u64,
    floor: u64,
    premine: String,
}

/// The floor follows the block time until someone writes their own.
fn floor_after(floor: u64, seconds_before: u64, seconds: u64) -> u64 {
    match floor == Rules::floor_for(seconds_before.saturating_mul(1_000)) {
        true => Rules::floor_for(seconds.saturating_mul(1_000)),
        false => floor,
    }
}

/// The short name of a coin, from the letters of its name.
fn ticker_of(name: &str) -> String {
    let letters: String = name.chars().filter(char::is_ascii_alphanumeric).take(TICKER_LETTERS).collect();
    match letters.is_empty() {
        true => NO_LETTERS.to_string(),
        false => letters.to_uppercase(),
    }
}

impl Default for CoinForm {
    fn default() -> Self {
        let rules = CoinConfig::olc().rules;
        Self {
            name: String::new(),
            ticker: String::new(),
            block_seconds: rules.block_time_ms / 1000,
            reward: rules.reward / COIN,
            halving: rules.halving_interval,
            difficulty: MIN_DIFFICULTY,
            floor: rules.min_difficulty,
            premine: String::new(),
        }
    }
}

impl CoinForm {
    fn basic_rows(&mut self, ui: &mut Ui, cx: &Cx) {
        let t = cx.t;
        row(ui, t, cx.say("coin.name"), cx.say("coin.name_about"), |ui| {
            field(ui, &mut self.name, cx.say("coin.name"), ui.available_width().min(200.0));
        });
        row(ui, t, cx.say("coin.block"), cx.say("coin.block_about"), |ui| {
            let items = BLOCK_CHOICES.map(|seconds| (seconds, format!("{seconds}")));
            let before = self.block_seconds;
            if let Some(seconds) = segments(ui, t, &items, Some(self.block_seconds)) {
                self.block_seconds = seconds;
                self.floor = floor_after(self.floor, before, seconds);
            }
        });
        row(ui, t, cx.say("coin.reward"), cx.say("coin.reward_about"), |ui| {
            let items = REWARD_CHOICES.map(|coins| (coins, format!("{coins}")));
            if let Some(coins) = segments(ui, t, &items, Some(self.reward)) {
                self.reward = coins;
            }
        });
    }

    fn expert_rows(&mut self, ui: &mut Ui, cx: &Cx) {
        let t = cx.t;
        let seconds_before = self.block_seconds;
        row(ui, t, cx.say("pro.ticker"), cx.say("pro.ticker_about"), |ui| {
            field(ui, &mut self.ticker, cx.say("pro.ticker"), 110.0);
        });
        let rows: [(&str, &str, &mut u64); 4] = [
            ("pro.block_seconds", "pro.block_seconds_about", &mut self.block_seconds),
            ("pro.halving", "pro.halving_about", &mut self.halving),
            ("pro.difficulty", "pro.difficulty_about", &mut self.difficulty),
            ("pro.floor", "pro.floor_about", &mut self.floor),
        ];
        for (name, about, value) in rows {
            row(ui, t, cx.say(name), cx.say(about), |ui| {
                number_field(ui, value, cx.say(name), 130.0);
            });
        }
        self.floor = floor_after(self.floor, seconds_before, self.block_seconds);
        row(ui, t, cx.say("pro.premine"), cx.say("pro.premine_about"), |ui| {
            field(ui, &mut self.premine, "0", 130.0);
        });
    }

    pub fn show(&mut self, ui: &mut Ui, cx: &Cx) {
        if self.name.is_empty() {
            self.name = cx.say("coin.default_name").to_string();
        }
        ui.spacing_mut().item_spacing.y = 0.0;
        self.basic_rows(ui, cx);
        if cx.expert() {
            self.expert_rows(ui, cx);
        }
    }

    /// The coin as filled in, once it can be made.
    pub fn coin(&self, cx: &Cx) -> Option<CoinConfig> {
        let name = self.name.trim();
        if name.is_empty() || self.block_seconds == 0 {
            return None;
        }
        let ticker = match self.ticker.trim() {
            "" => ticker_of(name),
            written => written.to_string(),
        };
        let premine = match self.premine.trim() {
            "" | "0" => Vec::new(),
            amount => vec![(parse_address(&cx.snap.node.address)?, parse_coins(amount)?)],
        };
        Some(new_coin(NewCoin {
            name: Some(name.to_string()),
            ticker: Some(ticker),
            block_time_ms: Some(self.block_seconds * 1000),
            reward: Some(self.reward),
            halving: Some(self.halving),
            difficulty: Some(self.difficulty),
            floor: Some(self.floor),
            message_in_block: true,
            genesis_time_ms: Some(unix_ms()),
            premine,
            ..NewCoin::default()
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::{floor_after, ticker_of};

    #[test]
    fn the_floor_follows_the_block_time_until_it_is_edited() {
        assert_eq!(floor_after(30_000, 30, 60), 60_000);
        assert_eq!(floor_after(12_345, 30, 60), 12_345);
    }

    #[test]
    fn a_coin_gets_a_short_name_from_its_name() {
        assert_eq!(ticker_of("quick coin"), "QUI");
        assert_eq!(ticker_of("우리 코인"), "NEW");
    }
}
