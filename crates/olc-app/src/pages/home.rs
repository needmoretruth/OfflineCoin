//! The cash book: how much there is, the three things a person does with it, and what happened.
use super::coins::history_card;
use super::money::{ReceiveSheet, SendSheet};
use super::{Cx, Page};
use crate::core::{unix_ms, Activity, ActivityKind, Command, NodeView};
use crate::ui::floating::sheet;
use crate::ui::paint::ink;
use crate::ui::theme::desktop_reduces_motion;
use crate::ui::tokens;
use crate::ui::widgets::{
    self, button, button_on, card, cell, dot, header_cell, meta, right_cell, section, Columns, Kind,
};
use crate::ui::{format, Icon, Tokens};
use egui::{vec2, Align, CornerRadius, FontId, Label, Layout, RichText, Sense, Ui};
use olc_core::Hash;

/// The ledger shows this much, and the rest stays one click away in Records.
const LEDGER_ROWS: usize = 40;

enum Open {
    Receive(ReceiveSheet),
    Send(SendSheet),
}

#[derive(Default)]
pub struct Home {
    open: Option<Open>,
    newest: Option<Hash>,
    lit_at: f64,
}

/// The large figure, which rolls to a new value over the length of a stamp.
fn balance_line(ui: &mut Ui, t: &Tokens, units: u64, ticker: &str, still: bool) {
    let id = ui.scope_id().with("balance");
    let target = units as f32;
    let seconds = if still { 0.0 } else { tokens::STAMP_SECONDS };
    let rolled = ui.ctx().animate_value_with_time(id, target, seconds);
    let shown = if (rolled - target).abs() < 1.0 { units } else { rolled as u64 };
    let figure = format::amount(shown);
    let font = FontId::new(tokens::AMOUNT, crate::ui::fonts::strong());
    let galley = ui.painter().layout_no_wrap(figure.clone(), font, t.text);
    let size = galley.size();
    let lit = ink(&galley, t.ink_top, t.ink_bottom);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
        ui.painter().galley(rect.min, lit, t.text);
        let said = format!("{figure} {ticker}");
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, said.as_str()));
        ui.with_layout(Layout::left_to_right(Align::Max), |ui| {
            ui.add_space(0.0);
            let text = RichText::new(ticker).size(tokens::TITLE).color(t.soft);
            ui.add(Label::new(text).selectable(false));
        });
    });
}

fn state_line(ui: &mut Ui, cx: &Cx) {
    let t = cx.t;
    let node = &cx.snap.node;
    let voice = cx.voice();
    ui.horizontal(|ui| {
        if let Some(target) = node.syncing_to {
            dot(ui, t.warn);
            let height = format::number(node.height.into());
            ui.label(cx.fill("home.catching_up", &[&height, &format::number(target.into())]));
        } else if node.mining {
            dot(ui, t.good);
            let speed = format::rate(voice, node.hashrate);
            let line = match node.seconds_per_block() {
                Some(seconds) => {
                    cx.fill("home.mining_block", &[&speed, &format::seconds(voice, seconds)])
                }
                None => cx.fill("home.mining", &[&speed]),
            };
            ui.label(line);
        } else {
            dot(ui, t.muted);
            ui.label(RichText::new(cx.say("home.not_mining")).color(t.soft));
        }
    });
}

fn kind_key(item: &Activity) -> &'static str {
    match item.kind {
        ActivityKind::Mined => "home.mined",
        ActivityKind::Received => "home.received",
        ActivityKind::Sent => "home.sent",
        ActivityKind::ToSelf => "home.to_self",
    }
}

impl Home {
    fn actions(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let t = cx.t;
        let node = &cx.snap.node;
        ui.horizontal(|ui| {
            let light = node.light.is_some();
            if node.mining {
                if button(ui, t, cx.say("home.stop"), Kind::Plain).clicked() {
                    cx.send(Command::SetMining(false));
                }
            } else if button_on(ui, t, cx.say("home.start"), Kind::Main, !light).clicked() {
                cx.send(Command::SetMining(true));
            }
            if button(ui, t, cx.say("home.receive"), Kind::Plain).clicked() {
                self.open = Some(Open::Receive(ReceiveSheet::for_address(&node.address)));
            }
            if button(ui, t, cx.say("home.send"), Kind::Plain).clicked() {
                self.open = Some(Open::Send(SendSheet::default()));
            }
        });
        if light_or_full(ui, cx) {
            *cx.go = Some("page.settings");
        }
    }

    fn ledger(&mut self, ui: &mut Ui, cx: &mut Cx, still: bool) {
        let t = cx.t;
        let node = &cx.snap.node;
        let now_time = ui.input(|input| input.time);
        let first = node.activity.first().map(|item| item.id);
        if first != self.newest {
            if self.newest.is_some() {
                self.lit_at = now_time;
            }
            self.newest = first;
        }
        let lit = ((now_time - self.lit_at) as f32 / tokens::STAMP_SECONDS).clamp(0.0, 1.0);
        if node.activity.is_empty() {
            // The button that starts mining stands right above this.
            let heading = cx.say("home.empty");
            let body = cx.say(if node.mining { "home.empty_mining" } else { "home.empty_body" });
            widgets::empty(ui, t, heading, body, None);
            return;
        }
        section(ui, cx.say("home.ledger"));
        ui.add_space(2.0);
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
        for (row, item) in node.activity.iter().take(LEDGER_ROWS).enumerate() {
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
    }

    fn sheets(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let t = cx.t;
        let closed = match &mut self.open {
            None => false,
            Some(Open::Receive(receive)) => {
                let (done, outside) = sheet(ui, t, "receive", tokens::SLIP_WIDTH, |ui| receive.show(ui, cx));
                done || outside
            }
            Some(Open::Send(send)) => {
                let (done, outside) = sheet(ui, t, "send", tokens::SLIP_WIDTH, |ui| send.show(ui, cx));
                done || outside
            }
        };
        if closed {
            self.open = None;
        }
    }
}

/// Says why mining cannot run or space ran out. True when the person wants to change that.
fn light_or_full(ui: &mut Ui, cx: &Cx) -> bool {
    let node = &cx.snap.node;
    let key = match () {
        () if node.light.is_some() => "home.light",
        () if node.storage_full => "home.full",
        () => return false,
    };
    ui.add_space(4.0);
    let open = ui
        .horizontal(|ui| {
            dot(ui, cx.t.warn);
            ui.label(cx.say(key));
            button(ui, cx.t, cx.say("home.open_settings"), Kind::Quiet).clicked()
        })
        .inner;
    light_limits(ui, cx);
    open
}

/// What a node without blocks cannot check, which decides how far its balance can be trusted.
fn light_limits(ui: &mut Ui, cx: &Cx) {
    let Some(light) = &cx.snap.node.light else {
        return;
    };
    if let Some(oldest) = light.pruned_from {
        ui.add_space(4.0);
        meta(ui, &cx.fill("home.light_pruned", &[&format::number(oldest.into())]), cx.t);
    }
    if light.unproven > 0 {
        ui.add_space(4.0);
        meta(ui, &cx.fill("home.light_unproven", &[&format::number(light.unproven as u128)]), cx.t);
    }
}

fn fact(ui: &mut Ui, t: &Tokens, name: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.set_min_height(26.0);
        ui.add(Label::new(RichText::new(name).color(t.muted)).selectable(false));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add(Label::new(value).truncate());
        });
    });
}

fn facts(ui: &mut Ui, cx: &Cx, node: &NodeView) {
    let t = cx.t;
    let number = |value: u64| format::number(value.into());
    card(ui, t, |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        if let Some(coin) = &node.coin {
            fact(ui, t, cx.say("home.coin"), &coin.name);
        }
        fact(ui, t, cx.say("home.height"), &number(node.height));
        fact(ui, t, cx.say("home.my_blocks"), &number(node.blocks_mined));
        fact(ui, t, cx.say("home.boards"), &number(cx.snap.boards.len() as u64));
        fact(ui, t, cx.say("home.others"), &number(node.peers as u64));
        if cx.expert() {
            fact(ui, t, cx.say("pro.difficulty"), &number(node.difficulty));
            fact(ui, t, cx.say("pro.mempool"), &number(node.pool as u64));
            fact(ui, t, cx.say("pro.utxo"), &number(node.unspent as u64));
            let tip = format::short_id(&node.tip);
            fact(ui, t, cx.say("pro.tip"), &tip);
        }
    });
}

impl Page for Home {
    fn title(&self) -> &'static str {
        "page.home"
    }

    fn icon(&self) -> Icon {
        Icon::Wallet
    }

    fn show(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let t = cx.t;
        let node = &cx.snap.node;
        let still = cx.settings.reduce_motion || desktop_reduces_motion();
        let wide = ui.available_width() >= tokens::COMPACT;
        meta(ui, cx.say("home.balance"), t);
        balance_line(ui, t, node.balance, cx.ticker(), still);
        // The line keeps its room, so the buttons stay put when the first block arrives.
        widgets::slot(ui, |ui| {
            ui.set_min_height(tokens::META_LINE);
            if node.maturing > 0 {
                meta(ui, &cx.fill("home.soon", &[&format::coins(node.maturing, cx.ticker())]), t);
            }
        });
        ui.add_space(tokens::GAP_WIDE - tokens::GAP);
        self.actions(ui, cx);
        ui.add_space(4.0);
        state_line(ui, cx);
        ui.add_space(tokens::GAP_PAGE - tokens::GAP);
        // A person starts here, so what stops this node shows here.
        widgets::slot(ui, |ui| history_card(ui, cx));
        if wide {
            let total = ui.available_width();
            let left = total - tokens::FACTS - tokens::GAP_PAGE;
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(vec2(left, 0.0), Layout::top_down(Align::Min), |ui| {
                    ui.set_width(left);
                    self.ledger(ui, cx, still);
                });
                ui.add_space(tokens::GAP_PAGE - ui.spacing().item_spacing.x);
                ui.allocate_ui_with_layout(vec2(tokens::FACTS, 0.0), Layout::top_down(Align::Min), |ui| {
                    ui.set_width(tokens::FACTS);
                    facts(ui, cx, &cx.snap.node);
                });
            });
        } else {
            self.ledger(ui, cx, still);
            ui.add_space(tokens::GAP_WIDE);
            facts(ui, cx, &cx.snap.node);
        }
        self.sheets(ui, cx);
    }
}
