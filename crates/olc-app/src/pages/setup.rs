//! The first run: three questions at most, then a working wallet.
use super::coin_new::CoinForm;
use super::settings::device_name;
use super::Cx;
use crate::core::{Command, StartCoin};
use crate::discovery::{CoinGroup, Reach};
use crate::settings::{Device, BOARD_CAP};
use crate::ui::format;
use crate::ui::paint::{surface, Surface};
use crate::ui::tokens;
use crate::ui::widgets::{self, button, button_on, meta, Kind};
use egui::{pos2, vec2, Sense, Ui};
use olc_core::CoinConfig;

const WIDTH: f32 = 620.0;
const CHOICE_HEIGHT: f32 = 62.0;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Step {
    #[default]
    Device,
    Coin,
    NewCoin,
    Mining,
}

#[derive(Default)]
pub struct Setup {
    step: Step,
    form: CoinForm,
    start: Option<StartCoin>,
}

fn device_about(device: Device) -> &'static str {
    match device {
        Device::Esp32 => "setup.esp32_about",
        Device::RaspberryPi => "setup.pi_about",
        Device::ThisComputer => "setup.this_computer_about",
        Device::TwoComputers => "setup.two_computers_about",
    }
}

/// A wide choice with a line under its name.
fn choice(ui: &mut Ui, cx: &Cx, name: &str, about: &str) -> bool {
    let t = cx.t;
    let size = vec2(ui.available_width(), CHOICE_HEIGHT);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let hover = response.hovered();
    let look = Surface {
        top: if hover { t.hover } else { t.card_top },
        bottom: if hover { t.hover } else { t.card_bottom },
        line: t.line,
        glint: t.glint,
        shadow: None,
        radius: tokens::RADIUS_CARD,
    };
    surface(ui.painter(), rect, &look, None);
    let left = rect.left() + f32::from(tokens::CARD_MARGIN);
    let name_font = egui::FontId::new(tokens::TEXT, crate::ui::fonts::strong());
    let about_font = egui::FontId::new(tokens::META, egui::FontFamily::Proportional);
    ui.painter().text(pos2(left, rect.top() + 13.0), egui::Align2::LEFT_TOP, name, name_font, t.text);
    ui.painter().text(pos2(left, rect.top() + 34.0), egui::Align2::LEFT_TOP, about, about_font, t.muted);
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
        widgets::title(ui, cx.say("setup.using"));
        meta(ui, cx.say("setup.change_later"), cx.t);
        ui.add_space(tokens::GAP);
        for device in Device::ALL {
            let name = cx.say(device_name(device));
            let cap = format::rate(cx.voice(), BOARD_CAP);
            let about = cx.fill(device_about(device), &[&cap]);
            if choice(ui, cx, name, &about) {
                cx.settings.apply_device(device);
                self.step = Step::Coin;
            }
        }
    }

    fn coin_step(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let t = cx.t;
        widgets::title(ui, cx.say("setup.coin"));
        for (coin, count, address) in found_coins(cx) {
            let about = cx.fill("setup.found_about", &[&coin.ticker, &count.to_string()]);
            if choice(ui, cx, &coin.name, &about) {
                self.start = Some(StartCoin::Join { coin, address });
                self.step = Step::Mining;
            }
        }
        let olc = CoinConfig::olc();
        if choice(ui, cx, &olc.name, cx.say("setup.olc_about")) {
            self.start = Some(StartCoin::Default);
            self.step = Step::Mining;
        }
        if choice(ui, cx, cx.say("coins.new"), cx.say("setup.new_coin_about")) {
            self.step = Step::NewCoin;
        }
        ui.add_space(tokens::GAP);
        if button(ui, t, cx.say("common.back"), Kind::Quiet).clicked() {
            self.step = Step::Device;
        }
    }

    fn new_coin_step(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let t = cx.t;
        widgets::title(ui, cx.say("coins.new"));
        ui.scope(|ui| self.form.show(ui, cx));
        ui.add_space(tokens::GAP_WIDE);
        let coin = self.form.coin(cx);
        ui.horizontal(|ui| {
            let next = button_on(ui, t, cx.say("common.next"), Kind::Main, coin.is_some());
            if let Some(coin) = coin.filter(|_| next.clicked()) {
                self.start = Some(StartCoin::New(coin));
                self.step = Step::Mining;
            }
            if button(ui, t, cx.say("common.back"), Kind::Quiet).clicked() {
                self.step = Step::Coin;
            }
        });
    }

    fn finish(&mut self, cx: &mut Cx) {
        cx.settings.setup_done = true;
        let start = self.start.take().unwrap_or(StartCoin::Default);
        cx.send(Command::FinishSetup(Box::new(cx.settings.clone()), start));
    }

    fn mining_step(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let t = cx.t;
        widgets::title(ui, cx.say("setup.mine"));
        let about = cx.fill("setup.mine_about", &[&format::rate(cx.voice(), cx.settings.hash_cap)]);
        if choice(ui, cx, cx.say("setup.mine_now"), &about) {
            cx.settings.mine_on_start = true;
            self.finish(cx);
        }
        if choice(ui, cx, cx.say("setup.mine_later"), cx.say("setup.mine_later_about")) {
            cx.settings.mine_on_start = false;
            self.finish(cx);
        }
        ui.add_space(tokens::GAP);
        if button(ui, t, cx.say("common.back"), Kind::Quiet).clicked() {
            self.step = Step::Coin;
        }
    }

    pub fn show(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let margin = ((ui.available_width() - WIDTH) / 2.0).max(0.0);
        ui.horizontal(|ui| {
            ui.add_space(margin);
            ui.vertical(|ui| {
                ui.set_width(WIDTH.min(ui.available_width()));
                ui.add_space(tokens::GAP_WIDE);
                match self.step {
                    Step::Device => self.device_step(ui, cx),
                    Step::Coin => self.coin_step(ui, cx),
                    Step::NewCoin => self.new_coin_step(ui, cx),
                    Step::Mining => self.mining_step(ui, cx),
                }
            });
        });
    }
}
