//! The form for a new coin. The first run and the Coins place share it. A person can press go
//! with what it starts with, and an expert has every setting a coin has.
use super::coin_rows::{bad_line, ms_to_seconds, rule_row, seconds_to_ms, text_of};
use super::Cx;
use crate::core::{new_coin, parse_coins, unix_ms, Bad, CoinField, FieldKind, NewCoin, Why, COIN_FIELDS};
use crate::ui::register::row;
use crate::ui::widgets::{field, segments, slot};
use egui::Ui;
use olc_core::coin::COIN;
use olc_core::CoinConfig;
use std::collections::HashMap;

const BLOCK_CHOICES: [u64; 3] = [10, 30, 60];
const REWARD_CHOICES: [u64; 3] = [1, 10, 50];
const TICKER_LETTERS: usize = 3;
const NO_LETTERS: &str = "NEW";
const BLOCK_TIME: &str = "rules.block_time_ms";
const REWARD: &str = "rules.reward";
const SMALL_FIELD: f32 = 90.0;

pub struct CoinForm {
    name: String,
    /// The default name goes in once, so a person can empty the box.
    named: bool,
    /// The block time as seconds, as a person types it. `typed` keeps it in milliseconds.
    seconds: String,
    /// What is typed, by coin field. A field left empty follows from the rest.
    typed: HashMap<&'static str, String>,
}

/// The short name of a coin, from the letters of its name.
fn ticker_of(name: &str) -> String {
    let letters: String = name.chars().filter(char::is_ascii_alphanumeric).take(TICKER_LETTERS).collect();
    match letters.is_empty() {
        true => NO_LETTERS.to_string(),
        false => letters.to_uppercase(),
    }
}

/// A bare amount in the form pays this wallet, which needs no typing of its own address.
fn premine_for(text: &str, own: &str) -> String {
    match parse_coins(text) {
        Some(0) => String::new(),
        Some(_) => format!("{own} {}", text.trim()),
        None => text.to_string(),
    }
}

impl Default for CoinForm {
    fn default() -> Self {
        let rules = CoinConfig::olc().rules;
        let mut form = Self {
            name: String::new(),
            named: false,
            seconds: String::new(),
            typed: HashMap::new(),
        };
        form.set_block_ms(rules.block_time_ms);
        form.typed.insert(REWARD, (rules.reward / COIN).to_string());
        form
    }
}

impl CoinForm {
    fn set_block_ms(&mut self, ms: u64) {
        self.seconds = ms_to_seconds(ms);
        self.typed.insert(BLOCK_TIME, ms.to_string());
    }

    /// The form as choices for the one function that makes a coin.
    fn choices(&self, own: &str, now: u64) -> Result<NewCoin, Bad> {
        let name = self.name.trim();
        if name.is_empty() {
            return Err(Bad { key: "name".into(), why: Why::Empty });
        }
        let mut choices = NewCoin::default()
            .with("name", name)
            .with("ticker", ticker_of(name))
            .with("genesis_time_ms", now.to_string());
        for (key, text) in &self.typed {
            let text = match *key {
                "premine" => premine_for(text, own),
                _ => text.clone(),
            };
            choices = choices.with(key, text);
        }
        Ok(choices)
    }

    fn made(&self, own: &str, now: u64) -> Result<CoinConfig, Bad> {
        new_coin(&self.choices(own, now)?)
    }

    fn name_row(&mut self, ui: &mut Ui, cx: &Cx) {
        row(ui, cx.t, cx.say("coin.name"), cx.say("coin.name_about"), |ui| {
            field(ui, &mut self.name, cx.say("coin.name"), ui.available_width().min(200.0));
        });
    }

    fn seconds_row(&mut self, ui: &mut Ui, cx: &Cx) {
        let t = cx.t;
        row(ui, t, cx.say("coin.block"), cx.say("coin.block_about"), |ui| {
            let items = BLOCK_CHOICES.map(|seconds| (seconds, format!("{seconds}")));
            let now = seconds_to_ms(&self.seconds).map(|ms| ms / 1000);
            if let Some(seconds) = segments(ui, t, &items, now) {
                self.set_block_ms(seconds.saturating_mul(1000));
            }
            if field(ui, &mut self.seconds, cx.say("coin.block"), SMALL_FIELD).changed() {
                let ms = seconds_to_ms(&self.seconds);
                let text = ms.map_or_else(|| self.seconds.clone(), |ms| ms.to_string());
                self.typed.insert(BLOCK_TIME, text);
            }
        });
    }

    fn reward_row(&mut self, ui: &mut Ui, cx: &Cx) {
        let t = cx.t;
        row(ui, t, cx.say("coin.reward"), cx.say("coin.reward_about"), |ui| {
            let items = REWARD_CHOICES.map(|coins| (coins * COIN, format!("{coins}")));
            let typed = self.typed.entry(REWARD).or_default();
            if let Some(units) = segments(ui, t, &items, parse_coins(typed)) {
                *typed = (units / COIN).to_string();
            }
            field(ui, typed, cx.say("coin.reward"), SMALL_FIELD);
        });
    }

    /// Every setting of a coin but the name and reward above, in the order of the table.
    fn expert_rows(&mut self, ui: &mut Ui, cx: &Cx, preview: &CoinConfig) {
        for field in COIN_FIELDS.iter().filter(|field| !matches!(field.key, "name" | "rules.reward")) {
            let text = self.typed.entry(field.key).or_default();
            let changed = rule_row(ui, cx, field, text, &hint_of(field, preview, cx));
            if changed && field.key == BLOCK_TIME {
                self.seconds = text.trim().parse().map_or_else(|_| String::new(), ms_to_seconds);
            }
        }
    }

    pub fn show(&mut self, ui: &mut Ui, cx: &Cx) {
        if !self.named {
            self.name = cx.say("coin.default_name").to_string();
            self.named = true;
        }
        ui.spacing_mut().item_spacing.y = 0.0;
        self.name_row(ui, cx);
        let (made, olc) = (self.made(&cx.snap.node.address, unix_ms()), CoinConfig::olc());
        if !cx.expert() {
            self.seconds_row(ui, cx);
        }
        self.reward_row(ui, cx);
        if cx.expert() {
            self.expert_rows(ui, cx, made.as_ref().unwrap_or(&olc));
        }
        slot(ui, |ui| bad_line(ui, cx, made.as_ref().err()));
    }

    /// The coin as filled in, once it can be made.
    pub fn coin(&self, cx: &Cx) -> Option<CoinConfig> {
        self.made(&cx.snap.node.address, unix_ms()).ok()
    }
}

/// What an empty box comes to, read from the coin it would make.
fn hint_of(field: &CoinField, preview: &CoinConfig, cx: &Cx) -> String {
    match (field.key, field.kind) {
        ("genesis_time_ms", _) => cx.say("pro.genesis_time_hint").to_string(),
        ("premine", _) => "0".to_string(),
        (_, FieldKind::Switch) => String::new(),
        _ => text_of(field, preview),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::COIN_FIELDS;
    use olc_wallet::format_address;

    const NOW: u64 = 5_000;

    fn own() -> String {
        format_address(&[9; 20])
    }

    fn named() -> CoinForm {
        CoinForm { name: "Quick Coin".into(), named: true, ..CoinForm::default() }
    }

    #[test]
    fn a_coin_gets_a_short_name_from_its_name() {
        assert_eq!(ticker_of("quick coin"), "QUI");
        assert_eq!(ticker_of("우리 코인"), "NEW");
    }

    #[test]
    fn the_form_as_it_opens_makes_a_coin_with_owner_defaults() {
        let coin = named().made(&own(), NOW).unwrap();
        let olc = CoinConfig::olc();
        assert_eq!(coin.name, "Quick Coin");
        assert_eq!(coin.ticker, "QUI");
        assert_eq!(coin.genesis_time_ms, NOW);
        assert_eq!(coin.rules.block_time_ms, olc.rules.block_time_ms);
        assert_eq!(coin.rules.reward, olc.rules.reward);
        assert_eq!(coin.rules.floor_from_height, 0);
        assert!(coin.message_in_block);
    }

    /// Whatever the table holds, the form takes it and the made coin has it.
    #[test]
    fn every_box_of_the_form_reaches_the_coin() {
        let samples: [(&str, String); 15] = [
            ("ticker", "WLK".into()),
            ("message", "우리 코인 첫 블록".into()),
            ("message_in_block", "off".into()),
            ("genesis_time_ms", "1234567".into()),
            ("initial_difficulty", "321".into()),
            ("premine", format!("{} 12.5", own())),
            ("rules.block_time_ms", "200".into()),
            ("rules.retarget_interval", "7".into()),
            ("rules.reward", "0.25".into()),
            ("rules.halving_interval", "11".into()),
            ("rules.coinbase_maturity", "13".into()),
            ("rules.max_block_bytes", "9000".into()),
            ("rules.min_fee", "0.001".into()),
            ("rules.min_difficulty", "17".into()),
            ("rules.floor_from_height", "19".into()),
        ];
        assert_eq!(samples.len() + 1, COIN_FIELDS.len(), "the name has its own box");
        let mut form = named();
        for field in COIN_FIELDS.iter().filter(|field| field.key != "name") {
            let (_, sample) = samples.iter().find(|(key, _)| *key == field.key).unwrap();
            form.typed.insert(field.key, sample.clone());
        }
        let coin = form.made(&own(), NOW).unwrap();
        for field in COIN_FIELDS.iter().filter(|field| field.key != "name") {
            let (_, sample) = samples.iter().find(|(key, _)| *key == field.key).unwrap();
            assert_eq!(&text_of(field, &coin), sample, "{}", field.key);
        }
    }

    #[test]
    fn typed_seconds_come_to_milliseconds() {
        let mut form = named();
        form.seconds = "0.2".into();
        form.typed.insert(BLOCK_TIME, seconds_to_ms("0.2").unwrap().to_string());
        assert_eq!(form.made(&own(), NOW).unwrap().rules.block_time_ms, 200);
        form.set_block_ms(86_400_000);
        assert_eq!(form.seconds, "86400");
        assert_eq!(form.made(&own(), NOW).unwrap().rules.block_time_ms, 86_400_000);
    }

    #[test]
    fn a_bare_premine_pays_this_wallet() {
        assert_eq!(premine_for("12.5", "addr"), "addr 12.5");
        assert_eq!(premine_for("0", "addr"), "");
        assert_eq!(premine_for("other 5", "addr"), "other 5");
        let mut form = named();
        form.typed.insert("premine", "3".into());
        assert_eq!(form.made(&own(), NOW).unwrap().premine, vec![([9; 20], 3 * COIN)]);
    }

    #[test]
    fn an_empty_name_and_a_bad_value_say_where_the_problem_is() {
        let mut form = CoinForm { named: true, ..CoinForm::default() };
        assert_eq!(form.made(&own(), NOW).unwrap_err().key, "name");
        form.name = "Coin".into();
        form.typed.insert(REWARD, "lots".into());
        let bad = form.made(&own(), NOW).unwrap_err();
        assert_eq!((bad.key.as_str(), bad.why), (REWARD, Why::NotAmount));
    }
}
