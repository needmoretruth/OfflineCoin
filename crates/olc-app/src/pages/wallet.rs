//! What this app holds, and sending it.
use super::{Cx, Page};
use crate::core::{parse_coins, ActivityKind, Command};
use crate::ui::format;
use crate::ui::tokens;
use crate::ui::widgets::{
    self, button, card, cell, field, header_cell, meta, mono, section, stats, Columns, Kind,
};
use crate::ui::Icon;
use egui::Ui;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Default)]
pub struct Wallet {
    to: String,
    amount: String,
}

fn age(cx: &Cx, time_ms: u64) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64);
    let minutes = now.saturating_sub(time_ms) / 60_000;
    match minutes {
        0 => cx.say("time.now").to_string(),
        1..=59 => cx.fill("time.minutes", &[&minutes.to_string()]),
        60..=2879 => cx.fill("time.hours", &[&(minutes / 60).to_string()]),
        _ => cx.fill("time.days", &[&(minutes / 1440).to_string()]),
    }
}

impl Wallet {
    fn send_form(&mut self, ui: &mut Ui, cx: &Cx) {
        let t = cx.t;
        section(ui, cx.say("wallet.send"));
        ui.horizontal(|ui| {
            field(ui, &mut self.to, cx.say("wallet.to"), 380.0);
            field(ui, &mut self.amount, cx.say("wallet.amount"), 130.0);
            let ready = !self.to.trim().is_empty() && parse_coins(&self.amount).is_some();
            let sent = widgets::button_on(ui, t, cx.say("wallet.send"), Kind::Main, ready);
            if sent.clicked() {
                cx.send(Command::Pay {
                    to: self.to.trim().to_string(),
                    amount: std::mem::take(&mut self.amount),
                });
            }
        });
    }
}

impl Page for Wallet {
    fn title(&self) -> &'static str {
        "page.wallet"
    }

    fn icon(&self) -> Icon {
        Icon::Wallet
    }

    fn show(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let t = cx.t;
        let node = &cx.snap.node;
        let ticker = node.coin.as_ref().map_or("", |coin| coin.ticker.as_str());
        widgets::title(ui, cx.say("page.wallet"));
        card(ui, t, |ui| {
            stats(
                ui,
                t,
                &[
                    (cx.say("wallet.balance"), format::coins(node.balance, ticker)),
                    (cx.say("wallet.spendable"), format::coins(node.spendable, ticker)),
                ],
            );
            ui.add_space(4.0);
            section(ui, cx.say("wallet.address"));
            ui.horizontal(|ui| {
                ui.label(mono(node.address.clone()));
                if button(ui, t, cx.say("wallet.copy"), Kind::Plain).clicked() {
                    ui.ctx().copy_text(node.address.clone());
                }
            });
        });
        ui.add_space(tokens::GAP);
        card(ui, t, |ui| self.send_form(ui, cx));
        ui.add_space(tokens::GAP);
        if node.activity.is_empty() {
            widgets::empty(ui, t, cx.say("wallet.no_activity"), cx.say("wallet.no_activity_body"));
            return;
        }
        card(ui, t, |ui| {
            section(ui, cx.say("wallet.activity"));
            let columns = Columns::new(ui, &[1.2, 1.4, 1.0, 1.0]);
            columns.row(ui, |ui, index| {
                let keys = ["column.kind", "wallet.amount", "column.height", "column.time"];
                header_cell(ui, t, cx.say(keys[index]));
            });
            for item in &node.activity {
                columns.row(ui, |ui, index| match index {
                    0 => cell(ui, cx.say(match item.kind {
                        ActivityKind::Mined => "wallet.mined",
                        ActivityKind::Received => "wallet.received",
                        ActivityKind::Sent => "wallet.sent",
                    })),
                    1 => cell(ui, format::coins(item.amount, ticker)),
                    2 => cell(ui, item.height.map_or_else(|| cx.say("wallet.waiting").to_string(), |height| format::number(height.into()))),
                    _ => {
                        meta(ui, &age(cx, item.time_ms), t);
                    }
                });
            }
        });
    }
}
