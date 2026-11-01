//! The two sheets that move money: one to receive with a code, one to send through a slip.
use super::Cx;
use crate::core::{parse_coins, Command};
use crate::ui::floating::{sheet_head, Slip};
use crate::ui::tokens::{self, Tokens};
use crate::ui::widgets::{button, button_on, field, meta, mono, Kind};
use crate::ui::format;
use egui::{vec2, Color32, CornerRadius, Label, Rect, Sense, Ui};
use olc_service::qr::{qr_grid, QrGrid};
use olc_wallet::parse_address;

const CODE: f32 = 216.0;
const QUIET_SQUARES: f32 = 3.0;

pub struct ReceiveSheet {
    address: String,
    grid: Option<QrGrid>,
}

/// Always dark on white, whatever the theme, so any camera can read it.
fn code(ui: &mut Ui, grid: &QrGrid, name: &str) {
    let (rect, response) = ui.allocate_exact_size(vec2(CODE, CODE), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(tokens::RADIUS_CARD), Color32::WHITE);
    let square = CODE / (grid.width as f32 + QUIET_SQUARES * 2.0);
    let start = rect.min + vec2(square * QUIET_SQUARES, square * QUIET_SQUARES);
    for y in 0..grid.width {
        for x in 0..grid.width {
            if grid.is_dark(x, y) {
                let at = start + vec2(x as f32 * square, y as f32 * square);
                let cell = Rect::from_min_size(at, vec2(square + 0.3, square + 0.3));
                painter.rect_filled(cell, CornerRadius::ZERO, Color32::from_rgb(0x0E, 0x10, 0x14));
            }
        }
    }
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, name));
}

impl ReceiveSheet {
    pub fn for_address(address: &str) -> Self {
        Self { address: address.to_string(), grid: qr_grid(address) }
    }

    /// True when it is finished.
    pub fn show(&mut self, ui: &mut Ui, cx: &Cx) -> bool {
        let t = cx.t;
        sheet_head(ui, cx.say("receive.title"));
        meta(ui, cx.say("receive.body"), t);
        ui.add_space(tokens::GAP);
        ui.vertical_centered(|ui| {
            if let Some(grid) = &self.grid {
                code(ui, grid, cx.say("receive.code"));
            }
        });
        ui.add_space(tokens::GAP);
        ui.add(Label::new(mono(self.address.clone())).wrap());
        ui.add_space(tokens::GAP);
        let mut done = false;
        ui.horizontal(|ui| {
            if button(ui, t, cx.say("receive.copy"), Kind::Main).clicked() {
                ui.ctx().copy_text(self.address.clone());
            }
            done = button(ui, t, cx.say("common.close"), Kind::Plain).clicked();
        });
        done
    }
}

#[derive(Default)]
pub struct SendSheet {
    to: String,
    amount: String,
    tried: bool,
}

fn problem(ui: &mut Ui, t: &Tokens, text: &str) {
    ui.colored_label(t.bad, text);
}

impl SendSheet {
    fn slip(&self, cx: &Cx, units: u64, fee: u64) -> Slip {
        let ticker = cx.ticker();
        let left = cx.snap.node.spendable.saturating_sub(units + fee);
        let mut rows = vec![(cx.say("send.to").to_string(), self.to.trim().to_string())];
        rows.push((cx.say("send.fee").to_string(), format::coins(fee, ticker)));
        rows.push((cx.say("send.left").to_string(), format::coins(left, ticker)));
        Slip {
            what: cx.say("send.slip").to_string(),
            amount: Some(format::coins(units, ticker)),
            rows,
            warning: Some(cx.say("send.warning").to_string()),
            sign: cx.say("send.sign").to_string(),
            cancel: cx.say("common.back").to_string(),
            danger: false,
            order: vec![Command::Pay {
                to: self.to.trim().to_string(),
                amount: self.amount.trim().to_string(),
            }],
        }
    }

    /// True when it is finished.
    pub fn show(&mut self, ui: &mut Ui, cx: &mut Cx) -> bool {
        let t = cx.t;
        let node = &cx.snap.node;
        let ticker = cx.ticker();
        let fee = node.coin.as_ref().map_or(0, |coin| coin.rules.min_fee);
        sheet_head(ui, cx.say("send.title"));
        let width = ui.available_width();
        let spendable = format::coins(node.spendable, ticker);
        meta(ui, &cx.fill("send.have", &[&spendable]), t);
        if node.spendable == 0 {
            meta(ui, cx.say("send.none"), t);
        }
        ui.add_space(4.0);
        field(ui, &mut self.to, cx.say("send.to_hint"), width);
        let address = parse_address(self.to.trim());
        if self.tried && address.is_none() {
            problem(ui, t, cx.say("send.bad_address"));
        }
        ui.horizontal(|ui| {
            let hint = cx.say("send.amount_hint");
            field(ui, &mut self.amount, hint, width - 100.0);
            if button(ui, t, cx.say("send.all"), Kind::Plain).clicked() {
                self.amount = format::amount(node.spendable.saturating_sub(fee));
            }
        });
        let units = parse_coins(self.amount.trim()).filter(|units| *units > 0);
        let enough = units.is_some_and(|units| units + fee <= node.spendable);
        if self.tried && units.is_none() {
            problem(ui, t, cx.say("send.bad_amount"));
        } else if self.tried && !enough {
            problem(ui, t, cx.say("send.too_much"));
        }
        ui.add_space(tokens::GAP);
        let mut done = false;
        ui.horizontal(|ui| {
            let filled = !self.to.trim().is_empty() && !self.amount.trim().is_empty();
            if button_on(ui, t, cx.say("send.next"), Kind::Main, filled).clicked() {
                self.tried = true;
                if let (Some(units), true, true) = (units, enough, address.is_some()) {
                    *cx.slip = Some(self.slip(cx, units, fee));
                    done = true;
                }
            }
            if button(ui, t, cx.say("common.cancel"), Kind::Plain).clicked() {
                done = true;
            }
        });
        done
    }
}
