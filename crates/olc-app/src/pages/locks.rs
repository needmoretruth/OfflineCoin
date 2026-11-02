//! Coins under a beta lock: writing a lock in words, paying to it and spending from it.
use super::{Cx, Page, Slipping};
use crate::core::host_locks::{
    check_lock, fault_of_words, hash_lock_for, name_of_coin, time_lock_for, two_of_two_for,
};
use crate::core::{Command, LockOrder};
use crate::ui::floating::{sheet, sheet_head};
use crate::ui::format;
use crate::ui::tokens;
use crate::ui::widgets::{
    self, button, button_on, field, meta, mono, section, segments, slot, words_box, Kind,
};
use crate::ui::Icon;
use egui::Ui;

const WIDE: f32 = 560.0;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Preset {
    #[default]
    Own,
    Hash,
    TwoOfTwo,
    Time,
}

/// Enough of a long key or address to tell it from another.
fn shortened(text: &str) -> String {
    let start: String = text.chars().take(12).collect();
    format!("{start}...")
}

/// A coin a person is spending.
struct Spending {
    coin: String,
    words: String,
    value: u64,
    unlock: String,
    to: String,
}

#[derive(Default)]
pub struct Locks {
    preset: Preset,
    /// What the preset is built from: a secret, the other public key or a height.
    given: String,
    words: String,
    amount: String,
    spending: Option<Spending>,
    passed: String,
}

impl Locks {
    fn own_key(cx: &Cx) -> String {
        let keys = &cx.snap.node.keys;
        keys.iter().find(|key| key.receiving).map_or_else(String::new, |key| key.public.clone())
    }

    fn composed(&self, cx: &Cx) -> Option<String> {
        let given = self.given.trim();
        match self.preset {
            Preset::Own => None,
            Preset::Hash => (!given.is_empty()).then(|| hash_lock_for(given)),
            Preset::TwoOfTwo => two_of_two_for(&Self::own_key(cx), given),
            Preset::Time => time_lock_for(given, &cx.snap.node.address),
        }
    }

    fn pick_preset(&mut self, ui: &mut Ui, cx: &Cx) {
        let items = [
            (Preset::Own, cx.say("lock.own").to_string()),
            (Preset::Hash, cx.say("lock.hash").to_string()),
            (Preset::TwoOfTwo, cx.say("lock.two_of_two").to_string()),
            (Preset::Time, cx.say("lock.time").to_string()),
        ];
        if let Some(preset) = segments(ui, cx.t, &items, Some(self.preset)) {
            self.preset = preset;
            self.given = match preset {
                Preset::Time => (cx.snap.node.height + 100).to_string(),
                _ => String::new(),
            };
            self.words = self.composed(cx).unwrap_or_default();
        }
    }

    fn preset_input(&mut self, ui: &mut Ui, cx: &Cx) {
        let (hint, about) = match self.preset {
            Preset::Own => return,
            Preset::Hash => ("lock.secret", "lock.hash_about"),
            Preset::TwoOfTwo => ("lock.other_key", "lock.two_of_two_about"),
            Preset::Time => ("lock.height", "lock.time_about"),
        };
        ui.add_space(8.0);
        if field(ui, &mut self.given, cx.say(hint), WIDE).changed() {
            self.words = self.composed(cx).unwrap_or_default();
        }
        ui.add_space(4.0);
        meta(ui, cx.say(about), cx.t);
        if self.preset == Preset::TwoOfTwo {
            let key = Self::own_key(cx);
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                meta(ui, &cx.fill("lock.my_key", &[&shortened(&key)]), cx.t);
                if button(ui, cx.t, cx.say("lock.copy"), Kind::Quiet).clicked() {
                    ui.ctx().copy_text(key.clone());
                }
            });
        }
    }

    fn cost_line(&self, ui: &mut Ui, cx: &Cx) -> bool {
        let checked = check_lock(&self.words);
        slot(ui, |ui| match (&checked, self.words.trim().is_empty()) {
            (_, true) => {}
            (Ok(cost), false) => {
                let fields = [cost.bytes.to_string(), cost.ops.to_string(), cost.signature_checks.to_string()];
                meta(ui, &cx.fill("lock.cost", &[&fields[0], &fields[1], &fields[2]]), cx.t);
            }
            (Err(error), false) => {
                let (key, detail) = fault_of_words(error);
                meta(ui, &cx.fill(key, &[&detail]), cx.t);
            }
        });
        checked.is_ok()
    }

    fn write(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let t = cx.t;
        section(ui, cx.say("lock.write"));
        self.pick_preset(ui, cx);
        self.preset_input(ui, cx);
        ui.add_space(8.0);
        if words_box(ui, &mut self.words, cx.say("lock.words_hint"), WIDE).changed() {
            self.preset = Preset::Own;
        }
        let good = self.cost_line(ui, cx);
        let paying = good && self.amount.trim().parse::<f64>().is_ok_and(|amount| amount > 0.0);
        ui.horizontal(|ui| {
            field(ui, &mut self.amount, cx.say("lock.amount"), 160.0);
            let go = button_on(ui, t, cx.say("lock.pay"), Kind::Main, paying);
            if go.clicked() && paying {
                let (words, amount) = (self.words.clone(), self.amount.trim().to_string());
                *cx.slip = Some(cx.slip_for(Slipping {
                    what: cx.say("lock.pay"),
                    rows: vec![
                        (cx.say("lock.amount"), format!("{amount} {}", cx.ticker())),
                        (cx.say("lock.words"), words.clone()),
                    ],
                    warning: cx.say("lock.pay_warning"),
                    sign: cx.say("lock.pay"),
                    danger: false,
                    order: vec![Command::Lock(LockOrder::Pay { words, amount })],
                }));
            }
        });
    }

    fn coins(&mut self, ui: &mut Ui, cx: &Cx) {
        let t = cx.t;
        section(ui, cx.say("lock.coins"));
        if cx.snap.node.locks.is_empty() {
            meta(ui, cx.say("lock.no_coins"), t);
        }
        for coin in &cx.snap.node.locks {
            ui.horizontal(|ui| {
                ui.set_max_width(ui.available_width());
                let amount = format::coins(coin.value, cx.ticker());
                ui.label(widgets::strong_text(amount));
                let spend = button(ui, t, cx.say("lock.spend"), Kind::Plain);
                ui.add(egui::Label::new(mono(coin.words.as_str())).truncate());
                if spend.clicked() {
                    self.spending = Some(Spending {
                        coin: name_of_coin(&coin.point),
                        words: coin.words.clone(),
                        value: coin.value,
                        unlock: coin.unlock.clone(),
                        to: String::new(),
                    });
                }
            });
        }
    }

    fn signing(&mut self, ui: &mut Ui, cx: &Cx) {
        let t = cx.t;
        section(ui, cx.say("lock.signing"));
        if let Some((text, who)) = &cx.snap.node.half_spend {
            let names: Vec<String> = who.iter().map(|address| shortened(address)).collect();
            meta(ui, &cx.fill("lock.waits_for", &[&names.join(", ")]), t);
            ui.add_space(4.0);
            if button(ui, t, cx.say("lock.copy_spend"), Kind::Quiet).clicked() {
                ui.ctx().copy_text(text.clone());
            }
            ui.add_space(8.0);
        }
        ui.horizontal(|ui| {
            field(ui, &mut self.passed, cx.say("lock.passed_hint"), WIDE - 136.0);
            let ready = !self.passed.trim().is_empty();
            if button_on(ui, t, cx.say("lock.sign"), Kind::Plain, ready).clicked() && ready {
                let text = self.passed.trim().to_string();
                cx.send(Command::Lock(LockOrder::Sign { text }));
                self.passed.clear();
            }
        });
    }

    fn spend_sheet(&mut self, ui: &mut Ui, cx: &Cx) {
        let t = cx.t;
        let Some(spending) = &mut self.spending else {
            return;
        };
        let ((go, cancel), outside) = sheet(ui, t, "spend-lock", tokens::SHEET_WIDE, |ui| {
            sheet_head(ui, cx.say("lock.spend"));
            ui.label(widgets::strong_text(format::coins(spending.value, cx.ticker())));
            ui.add_space(4.0);
            ui.add(egui::Label::new(mono(spending.words.as_str())).wrap());
            ui.add_space(tokens::GAP);
            let width = ui.available_width();
            words_box(ui, &mut spending.unlock, cx.say("lock.unlock_hint"), width);
            ui.add_space(4.0);
            meta(ui, cx.say("lock.unlock_about"), t);
            ui.add_space(8.0);
            field(ui, &mut spending.to, cx.say("lock.to_hint"), width);
            ui.add_space(tokens::GAP_WIDE);
            let mut pressed = (false, false);
            ui.horizontal(|ui| {
                pressed.0 = button(ui, t, cx.say("lock.spend"), Kind::Main).clicked();
                pressed.1 = button(ui, t, cx.say("common.cancel"), Kind::Plain).clicked();
            });
            pressed
        });
        if go {
            cx.send(Command::Lock(LockOrder::Spend {
                coin: spending.coin.clone(),
                to: spending.to.trim().to_string(),
                unlock: spending.unlock.trim().to_string(),
            }));
        }
        if go || cancel || outside {
            self.spending = None;
        }
    }
}

impl Page for Locks {
    fn title(&self) -> &'static str {
        "page.locks"
    }

    fn icon(&self) -> Icon {
        Icon::Lock
    }

    fn expert_only(&self) -> bool {
        true
    }

    fn show(&mut self, ui: &mut Ui, cx: &mut Cx) {
        widgets::title(ui, cx.say("page.locks"));
        if cx.snap.node.light.is_some() {
            meta(ui, cx.say("lock.needs_blocks"), cx.t);
            return;
        }
        self.write(ui, cx);
        ui.add_space(tokens::GAP_WIDE);
        self.coins(ui, cx);
        ui.add_space(tokens::GAP_WIDE);
        self.signing(ui, cx);
        self.spend_sheet(ui, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_preset_makes_words_that_check() {
        let key = "02".to_string() + &"11".repeat(32);
        let address = format!("olc{}", "22".repeat(20));
        for words in [
            Some(hash_lock_for("a secret")),
            two_of_two_for(&key, &key),
            time_lock_for("500", &address),
        ] {
            assert!(check_lock(&words.unwrap()).is_ok());
        }
    }
}
