//! What happened to the wallet's coins, newest first. Home shows the last few rows and the Wallet
//! all it holds. The newest row lights up for a moment when it arrives.
use super::{Cx, Slipping};
use crate::core::{unix_ms, Activity, ActivityKind, Command, LockOrder};
use crate::ui::tokens;
use crate::ui::widgets::{self, button, cell, header_cell, right_cell, Columns, Kind};
use crate::ui::format;
use egui::{vec2, Align, CornerRadius, Layout, RichText, Ui};
use olc_core::Hash;

#[derive(Default)]
pub struct Ledger {
    newest: Option<Hash>,
    lit_at: f64,
}

fn kind_key(item: &Activity) -> &'static str {
    match item.kind {
        ActivityKind::Mined => "home.mined",
        ActivityKind::Received => "home.received",
        ActivityKind::Sent => "home.sent",
        ActivityKind::ToSelf => "home.to_self",
    }
}

impl Ledger {
    /// The rows, at most `rows` of them. Returns false when there is nothing to show yet.
    pub fn show(&mut self, ui: &mut Ui, cx: &mut Cx, rows: usize, still: bool) -> bool {
        let t = cx.t;
        let node = &cx.snap.node;
        if node.activity.is_empty() {
            return false;
        }
        let now_time = ui.input(|input| input.time);
        let first = node.activity.first().map(|item| item.id);
        if first != self.newest {
            if self.newest.is_some() {
                self.lit_at = now_time;
            }
            self.newest = first;
        }
        let lit = ((now_time - self.lit_at) as f32 / tokens::STAMP_SECONDS).clamp(0.0, 1.0);
        let now = unix_ms();
        let voice = cx.voice();
        let ticker = cx.ticker().to_string();
        let columns = Columns::new(ui, &[1.0, 1.5, 1.4]);
        columns.row(ui, |ui, index| {
            let key = ["home.when", "home.what", "home.amount"][index];
            if index == 2 {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| header_cell(ui, t, cx.say(key)));
            } else {
                header_cell(ui, t, cx.say(key));
            }
        });
        widgets::divider(ui, t);
        let mut speed = None;
        for (row, item) in node.activity.iter().take(rows).enumerate() {
            if row == 0 && lit < 1.0 && !still {
                let rect = egui::Rect::from_min_size(ui.cursor().min, vec2(ui.available_width(), tokens::ROW - 8.0));
                let wash = t.good.gamma_multiply(0.14 * (1.0 - lit));
                ui.painter().rect_filled(rect, CornerRadius::same(tokens::RADIUS_CONTROL), wash);
                ui.ctx().request_repaint();
            }
            columns.row(ui, |ui, index| match index {
                0 => {
                    let text = RichText::new(format::ago(voice, now, item.time_ms)).color(t.soft);
                    cell(ui, text);
                }
                1 => {
                    cell(ui, cx.say(kind_key(item)));
                    if item.height.is_none() {
                        widgets::tag(ui, cx.say("home.waiting"), t.warn, t);
                        if item.kind == ActivityKind::Sent && button(ui, t, cx.say("home.speed_up"), Kind::Quiet).clicked() {
                            speed = Some(cx.slip_for(speed_up_slip(cx, item)));
                        }
                    }
                }
                _ => {
                    let sign = match item.kind {
                        // Coins that stay in this wallet are neither gained nor lost.
                        ActivityKind::ToSelf => "",
                        _ if item.amount == 0 => "",
                        ActivityKind::Sent => "-",
                        _ => "+",
                    };
                    let figure = format!("{sign}{}", format::coins(item.amount, &ticker));
                    right_cell(ui, widgets::medium_text(figure));
                }
            });
        }
        if speed.is_some() {
            *cx.slip = speed;
        }
        true
    }
}

/// The slip before a waiting payment goes again with a higher fee.
/// Shows the fee now and the fee after, as Bitcoin Core's bump fee dialog does, worked out by the
/// same code that then sends it. Expert mode also names the payment.
fn speed_up_slip(cx: &Cx, item: &Activity) -> Slipping {
    let id: String = item.id.iter().map(|byte| format!("{byte:02x}")).collect();
    let quote = cx.snap.node.wallet.speed.iter().find(|quote| quote.id == item.id);
    let fee = |units: Option<u64>| units.map_or_else(|| "?".to_string(), |units| format::coins(units, cx.ticker()));
    let mut rows = vec![
        (cx.say("speed.amount"), format::coins(item.amount, cx.ticker())),
        (cx.say("speed.fee_now"), fee(quote.map(|quote| quote.now))),
        (cx.say("speed.fee_new"), fee(quote.map(|quote| quote.new))),
    ];
    if cx.expert() {
        rows.push((cx.say("pro.speed_id"), format::short_id(&item.id)));
    }
    Slipping {
        what: cx.say("speed.title"),
        rows,
        warning: cx.say("speed.warning"),
        sign: cx.say("speed.sign"),
        danger: false,
        order: vec![Command::Lock(LockOrder::SpeedUp { id })],
    }
}
