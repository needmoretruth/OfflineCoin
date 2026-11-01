//! The first run: what a person is using, then a coin to start or one to join.
use super::settings::device_name;
use super::Cx;
use crate::core::{parse_coins, Command, StartCoin};
use crate::discovery::{CoinGroup, Reach};
use crate::settings::{Device, NEW_COIN_DIFFICULTY};
use crate::ui::format;
use crate::ui::paint::{surface, Surface};
use crate::ui::tokens;
use crate::ui::widgets::{
    self, button, button_on, card, field, meta, number_field,
    switch_row, Kind,
};
use egui::{pos2, vec2, Label, Sense, Ui};
use olc_core::coin::COIN;
use olc_core::CoinConfig;
use olc_wallet::parse_address;

const WIDTH: f32 = 620.0;
const CHOICE_HEIGHT: f32 = 62.0;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Step {
    Device,
    Coin,
}

pub struct Setup {
    step: Step,
    new_coin: bool,
    name: String,
    ticker: String,
    block_seconds: u64,
    reward: u64,
    halving: u64,
    difficulty: u64,
    premine: String,
}

impl Default for Setup {
    fn default() -> Self {
        let rules = CoinConfig::olc().rules;
        Self {
            step: Step::Device,
            new_coin: false,
            name: String::new(),
            ticker: String::new(),
            block_seconds: rules.block_time_ms / 1000,
            reward: rules.reward / COIN,
            halving: rules.halving_interval,
            difficulty: NEW_COIN_DIFFICULTY,
            premine: String::new(),
        }
    }
}

fn device_about(device: Device) -> &'static str {
    match device {
        Device::Esp32 => "setup.esp32_about",
        Device::RaspberryPi => "setup.pi_about",
        Device::ThisComputer => "setup.this_computer_about",
        Device::ComputerAndPhone => "setup.computer_phone_about",
        Device::TwoComputers => "setup.two_computers_about",
    }
}

fn form_label(ui: &mut Ui, text: &str) {
    let area = vec2(190.0, tokens::CONTROL);
    ui.allocate_ui_with_layout(area, egui::Layout::left_to_right(egui::Align::Center), |ui| {
        ui.set_width(area.x);
        ui.add(Label::new(text).wrap());
    });
}

/// A wide choice with a line under its name.
fn choice(ui: &mut Ui, cx: &Cx, name: &str, about: &str, current: bool) -> bool {
    let t = cx.t;
    let size = vec2(ui.available_width(), CHOICE_HEIGHT);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let hover = response.hovered();
    let look = Surface {
        top: if hover { t.hover } else { t.card_top },
        bottom: if hover { t.hover } else { t.card_bottom },
        line: if current { t.text } else { t.line },
        glint: t.glint,
        shadow: None,
        radius: tokens::RADIUS_CARD,
    };
    surface(ui.painter(), rect, &look, None);
    let left = rect.left() + tokens::CARD_MARGIN as f32;
    let name_font = egui::FontId::new(tokens::TEXT, crate::ui::fonts::strong());
    let about_font = egui::FontId::new(tokens::META, egui::FontFamily::Proportional);
    ui.painter().text(pos2(left, rect.top() + 13.0), egui::Align2::LEFT_TOP, name, name_font, t.text);
    ui.painter().text(pos2(left, rect.top() + 34.0), egui::Align2::LEFT_TOP, about, about_font, t.muted);
    if current {
        ui.painter()
            .circle_filled(pos2(rect.right() - 22.0, rect.center().y), 5.0, t.text);
    }
    if response.has_focus() {
        ui.painter().rect_stroke(
            rect.expand(1.0),
            tokens::RADIUS_CARD,
            egui::Stroke::new(1.5, t.info),
            egui::StrokeKind::Outside,
        );
    }
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, name));
    response.clicked()
}

/// The coins found around, with the address to reach each by.
fn found_coins(cx: &Cx) -> Vec<(CoinConfig, usize, Option<String>)> {
    let others = |group: &&CoinGroup| group.identity.is_some();
    cx.snap
        .groups
        .iter()
        .filter(others)
        .filter_map(|group| {
            let entries = group.chains.iter().flat_map(|chain| &chain.entries);
            let around: Vec<_> = entries.filter(|entry| entry.reach != Reach::ThisApp).collect();
            let first = around.first()?;
            let coin = group.identity.as_ref()?.coin.clone();
            Some((coin, around.len(), first.address.clone()))
        })
        .collect()
}

impl Setup {
    fn device_step(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let t = cx.t;
        widgets::title(ui, cx.say("setup.using"));
        for device in Device::ALL {
            let name = cx.say(device_name(device));
            let about = cx.say(device_about(device));
            if choice(ui, cx, name, about, cx.settings.device == device) {
                cx.settings.apply_device(device);
            }
        }
        ui.add_space(tokens::GAP);
        meta(ui, cx.say("setup.change_later"), t);
        ui.add_space(tokens::GAP);
        if button(ui, t, cx.say("setup.next"), Kind::Main).clicked() {
            self.step = Step::Coin;
        }
    }

    fn new_coin_form(&mut self, ui: &mut Ui, cx: &Cx) {
        let rows: [(&str, &mut u64); 4] = [
            ("setup.block_seconds", &mut self.block_seconds),
            ("setup.reward", &mut self.reward),
            ("setup.halving", &mut self.halving),
            ("setup.difficulty", &mut self.difficulty),
        ];
        ui.horizontal(|ui| {
            field(ui, &mut self.name, cx.say("setup.coin_name"), 220.0);
            field(ui, &mut self.ticker, cx.say("setup.ticker"), 110.0);
        });
        for (key, value) in rows {
            ui.horizontal(|ui| {
                form_label(ui, cx.say(key));
                number_field(ui, value, 130.0);
            });
        }
        ui.horizontal(|ui| {
            form_label(ui, cx.say("setup.premine"));
            field(ui, &mut self.premine, "0", 130.0);
        });
    }

    fn coin_of_form(&self, cx: &Cx) -> Option<CoinConfig> {
        let name = self.name.trim();
        let ticker = self.ticker.trim();
        if name.is_empty() || ticker.is_empty() || self.block_seconds == 0 {
            return None;
        }
        let mut coin = CoinConfig::olc();
        coin.name = name.to_string();
        coin.ticker = ticker.to_string();
        coin.message = format!("{name} genesis");
        coin.genesis_time_ms = crate::core::unix_ms();
        coin.initial_difficulty = self.difficulty;
        coin.rules.block_time_ms = self.block_seconds * 1000;
        coin.rules.reward = self.reward * COIN;
        coin.rules.halving_interval = self.halving;
        let premine = self.premine.trim();
        if !premine.is_empty() && premine != "0" {
            let amount = parse_coins(premine)?;
            let address = parse_address(&cx.snap.node.address)?;
            coin.premine = vec![(address, amount)];
        }
        Some(coin)
    }

    fn finish(cx: &mut Cx, start: StartCoin) {
        cx.settings.setup_done = true;
        cx.send(Command::FinishSetup(Box::new(cx.settings.clone()), start));
    }

    fn coin_step(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let t = cx.t;
        widgets::title(ui, cx.say("setup.coin"));
        for (coin, count, address) in found_coins(cx) {
            let about = cx.fill("setup.found_about", &[&coin.ticker, &count.to_string()]);
            if choice(ui, cx, &coin.name, &about, false) {
                Self::finish(cx, StartCoin::Join { coin, address });
            }
        }
        let olc = CoinConfig::olc();
        if choice(ui, cx, &olc.name, cx.say("setup.olc_about"), false) {
            Self::finish(cx, StartCoin::Default);
        }
        if choice(ui, cx, cx.say("setup.new_coin"), cx.say("setup.new_coin_about"), self.new_coin) {
            self.new_coin = !self.new_coin;
        }
        if self.new_coin {
            card(ui, t, |ui| {
                self.new_coin_form(ui, cx);
                let coin = self.coin_of_form(cx);
                let ready = coin.is_some();
                let start = button_on(ui, t, cx.say("setup.start_coin"), Kind::Main, ready);
                if let Some(coin) = coin.filter(|_| start.clicked()) {
                    Self::finish(cx, StartCoin::New(coin));
                }
            });
        }
        ui.add_space(tokens::GAP);
        let cap = format::rate(cx.settings.hash_cap);
        let label = cx.fill("setup.mine_at", &[&cap]);
        switch_row(ui, t, &mut cx.settings.mine_on_start, &label);
        ui.add_space(tokens::GAP);
        if button(ui, t, cx.say("setup.back"), Kind::Quiet).clicked() {
            self.step = Step::Device;
        }
    }

    pub fn show(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let margin = ((ui.available_width() - WIDTH) / 2.0).max(0.0);
        ui.horizontal(|ui| {
            ui.add_space(margin);
            ui.vertical(|ui| {
                ui.set_width(WIDTH.min(ui.available_width()));
                ui.add_space(tokens::GAP_WIDE * 2.0);
                match self.step {
                    Step::Device => self.device_step(ui, cx),
                    Step::Coin => self.coin_step(ui, cx),
                }
            });
        });
    }
}
