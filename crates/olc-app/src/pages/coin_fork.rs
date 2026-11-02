//! Changing the running coin's rules. The form opens with the rules the coin has, and a person
//! changes what they want from a height they choose. Everything before it stays as it is.
use super::coin_rows::{bad_line, rule_row, text_of, words_of};
use super::{Cx, Slipping};
use crate::core::{fork_coin, first_fork_height, Bad, Command, Why, COIN_FIELDS};
use crate::ui::floating::Slip;
use crate::ui::format;
use crate::ui::register::row;
use crate::ui::widgets::{field, slot};
use egui::Ui;
use olc_core::CoinConfig;
use std::collections::HashMap;

const HEIGHT_FIELD: f32 = 200.0;
const HEIGHT_KEY: &str = "from_height";

pub struct ForkForm {
    from: String,
    typed: HashMap<&'static str, String>,
    /// What each rule held when the form opened, to tell what a person changed.
    before: HashMap<&'static str, String>,
}

impl ForkForm {
    pub fn new(coin: &CoinConfig, tip: u64) -> Self {
        let from = first_fork_height(coin, tip);
        let mut draft = coin.clone();
        draft.rules = coin.rules_at(from).clone();
        let rules = COIN_FIELDS.iter().filter(|field| field.is_rule());
        let before: HashMap<_, _> = rules.map(|field| (field.key, text_of(field, &draft))).collect();
        Self { from: from.to_string(), typed: before.clone(), before }
    }

    fn changes(&self) -> Vec<(String, String)> {
        let rules = COIN_FIELDS.iter().filter(|field| field.is_rule());
        let changed = rules.filter(|field| self.typed.get(field.key) != self.before.get(field.key));
        changed.map(|field| (field.key.to_string(), self.typed[field.key].clone())).collect()
    }

    fn from_height(&self) -> Result<u64, Bad> {
        let bad = || Bad { key: HEIGHT_KEY.into(), why: Why::NotNumber };
        self.from.trim().replace(',', "").parse().map_err(|_| bad())
    }

    /// The command that makes the change, once the form holds a change that can be made.
    pub fn order(&self, coin: &CoinConfig, tip: u64) -> Result<Command, Bad> {
        let from_height = self.from_height()?;
        let changes = self.changes();
        fork_coin(coin, from_height, tip, &changes)?;
        Ok(Command::Fork { at: Some(from_height), changes })
    }

    pub fn show(&mut self, ui: &mut Ui, cx: &Cx, coin: &CoinConfig, tip: u64) {
        ui.spacing_mut().item_spacing.y = 0.0;
        let bad = self.order(coin, tip).err();
        row(ui, cx.t, cx.say("pro.fork_from"), cx.say("pro.fork_from_about"), |ui| {
            field(ui, &mut self.from, cx.say("pro.fork_from"), ui.available_width().min(HEIGHT_FIELD));
        });
        for rule in COIN_FIELDS.iter().filter(|field| field.is_rule()) {
            let text = self.typed.entry(rule.key).or_default();
            let hint = self.before.get(rule.key).cloned().unwrap_or_default();
            rule_row(ui, cx, rule, text, &hint);
        }
        slot(ui, |ui| bad_line(ui, cx, bad.as_ref()));
    }

    /// What a person reads before the node leaves everyone on the old rules.
    pub fn slip(&self, cx: &Cx, coin: &CoinConfig, tip: u64, order: Command) -> Slip {
        let from = self.from_height().unwrap_or_else(|_| first_fork_height(coin, tip));
        let mut rows = vec![(cx.say("pro.fork_from"), format::number(from.into()))];
        for (key, text) in self.changes() {
            let name = cx.say(words_of(&key).0);
            let before = self.before.get(key.as_str()).cloned().unwrap_or_default();
            rows.push((name, cx.fill("pro.fork_change", &[&before, &text])));
        }
        cx.slip_for(Slipping {
            what: cx.say("pro.fork"),
            rows,
            warning: cx.say("pro.fork_warning"),
            sign: cx.say("pro.fork_go"),
            danger: true,
            order: vec![order],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use olc_core::coin::COIN;

    fn change(form: &mut ForkForm, key: &'static str, text: &str) {
        form.typed.insert(key, text.to_string());
    }

    #[test]
    fn the_form_opens_with_the_coin_as_it_runs_and_asks_for_a_change() {
        let coin = CoinConfig::olc();
        let form = ForkForm::new(&coin, 41);
        assert_eq!(form.from, "42");
        assert_eq!(form.typed["rules.block_time_ms"], coin.rules.block_time_ms.to_string());
        assert_eq!(form.order(&coin, 41).err().unwrap().why, Why::NoChange);
    }

    #[test]
    fn every_rule_in_the_form_reaches_the_order() {
        let coin = CoinConfig::olc();
        let mut form = ForkForm::new(&coin, 41);
        change(&mut form, "rules.reward", "7");
        change(&mut form, "rules.block_time_ms", "200");
        let Command::Fork { at, changes } = form.order(&coin, 41).unwrap() else {
            panic!("no fork");
        };
        assert_eq!(at, Some(42));
        assert_eq!(changes.len(), 2);
        let forked = fork_coin(&coin, 42, 41, &changes).unwrap();
        assert_eq!(forked.rules_at(42).reward, 7 * COIN);
        assert_eq!(forked.rules_at(42).block_time_ms, 200);
        assert_eq!(forked.rules_at(41).reward, coin.rules.reward);
    }

    #[test]
    fn a_height_already_passed_says_so() {
        let coin = CoinConfig::olc();
        let mut form = ForkForm::new(&coin, 41);
        change(&mut form, "rules.reward", "7");
        form.from = "40".into();
        assert_eq!(form.order(&coin, 41).err().unwrap().why, Why::AtLeast(42));
        form.from = "soon".into();
        assert_eq!(form.order(&coin, 41).err().unwrap().why, Why::NotNumber);
    }
}
