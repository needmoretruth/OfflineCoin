//! One history row opened: every fact of the payment, as Bitcoin Core's transaction details
//! give them, with the coins in and out and which of them are this wallet's.
use super::history::{blocks_to_ripe, confirmations, kind_key, label_of, signed_amount, status_cell, SETTLED};
use super::{Cx, Slipping};
use crate::core::{unix_ms, Activity, ActivityKind, Command, Explored, LockOrder, Personal, TxRow, WalletOrder};
use crate::ui::explain::{self, Choice};
use crate::ui::floating::sheet_head;
use crate::ui::tokens;
use crate::ui::widgets::{self, button, field, meta, mono, section, Kind};
use crate::ui::format;
use egui::{Grid, Label, RichText, Ui};

/// The height of about six coin lines; a longer list scrolls.
const COINS_SHOWN: f32 = 6.0 * 42.0;

pub struct Detail {
    item: Activity,
    name: String,
    named: bool,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn is_mine(cx: &Cx, address: &str) -> bool {
    !address.is_empty() && cx.snap.node.keys.iter().any(|key| key.address == address)
}

/// The payment as the explorer read it, once it has.
fn read_tx<'a>(cx: &'a Cx, id: &olc_core::Hash) -> Option<&'a TxRow> {
    match &cx.snap.node.explored {
        Explored::Tx { tx, .. } if tx.id == *id => Some(tx),
        Explored::Block(block) => block.txs.iter().find(|tx| tx.id == *id),
        _ => None,
    }
}

/// One coin in or out: amount, address or what it is, and a tag when it is this wallet's.
fn coin_line(ui: &mut Ui, cx: &Cx, value: Option<u64>, address: &str, other: &str) {
    let t = cx.t;
    ui.horizontal(|ui| {
        let amount = value.map_or_else(|| "?".to_string(), |value| format::coins(value, cx.ticker()));
        ui.label(widgets::medium_text(amount));
        let shown = if address.is_empty() { other.to_string() } else { format::short_address(address) };
        ui.add(Label::new(mono(shown)).truncate());
        if is_mine(cx, address) {
            widgets::tag(ui, cx.say("detail.mine"), t.good, t);
        }
        let label = cx.snap.node.wallet.labels.get(address);
        if let Some(label) = label {
            meta(ui, label, t);
        }
    });
}

impl Detail {
    pub fn new(item: Activity) -> Self {
        Self { item, name: String::new(), named: false }
    }

    fn facts(&self, ui: &mut Ui, cx: &mut Cx) -> bool {
        let t = cx.t;
        let item = &self.item;
        let ticker = cx.ticker().to_string();
        let tip = cx.snap.node.height;
        let mut leave = false;
        Grid::new("detail-facts").num_columns(2).min_col_width(110.0).spacing([tokens::GAP_WIDE, 2.0]).show(ui, |ui| {
            meta(ui, cx.say("detail.when"), t);
            ui.label(format!("{} · {}", format::moment(item.time_ms), format::ago(cx.voice(), unix_ms(), item.time_ms)));
            ui.end_row();
            meta(ui, cx.say("detail.amount"), t);
            ui.label(widgets::medium_text(signed_amount(item, &ticker)));
            ui.end_row();
            meta(ui, cx.say("detail.fee"), t);
            match item.fee.or_else(|| read_tx(cx, &item.id).and_then(|tx| tx.fee)) {
                Some(fee) => ui.label(format::coins(fee, &ticker)),
                None => ui.label(RichText::new(cx.say("detail.fee_unknown")).color(t.soft)),
            };
            ui.end_row();
            meta(ui, cx.say("detail.status"), t);
            ui.horizontal(|ui| {
                status_cell(ui, cx, item);
                if let Some(count) = confirmations(item, tip).filter(|_| !cx.expert()) {
                    meta(ui, &cx.fill("detail.blocks_on_top", &[&format::number(count.into()), &SETTLED.to_string()]), t);
                }
            });
            ui.end_row();
            if let Some(height) = item.height {
                meta(ui, cx.say("detail.block"), t);
                ui.horizontal(|ui| {
                    ui.label(format::number(height.into()));
                    if button(ui, t, cx.say("detail.open_block"), Kind::Quiet).clicked() {
                        cx.send(Command::Own(Personal::Explore(height.to_string())));
                        *cx.go = Some("page.records");
                        leave = true;
                    }
                });
                ui.end_row();
            }
            meta(ui, cx.say("detail.id"), t);
            ui.horizontal(|ui| {
                ui.add(Label::new(mono(format::short_id(&item.id))));
                if button(ui, t, cx.say("detail.copy"), Kind::Quiet).clicked() {
                    ui.ctx().copy_text(hex(&item.id));
                }
            });
            ui.end_row();
            meta(ui, cx.say("detail.size"), t);
            ui.label(cx.fill("detail.bytes", &[&format::number(item.size.into())]));
            ui.end_row();
        });
        leave
    }

    fn coins(&self, ui: &mut Ui, cx: &Cx) {
        let t = cx.t;
        if self.item.height.is_none() {
            meta(ui, cx.say("detail.coins_later"), t);
            return;
        }
        let Some(tx) = read_tx(cx, &self.item.id) else {
            meta(ui, cx.say("detail.reading"), t);
            return;
        };
        // Spent on the left, made on the right, as a block explorer draws a payment; each list
        // scrolls on its own past about six coins.
        let list = |ui: &mut Ui, salt: &str, lines: &dyn Fn(&mut Ui)| {
            egui::ScrollArea::vertical().id_salt(salt).max_height(COINS_SHOWN).auto_shrink([false, true]).show(ui, lines);
        };
        ui.columns(2, |columns| {
            section(&mut columns[0], &cx.fill("detail.inputs", &[&tx.ins.len().to_string()]));
            list(&mut columns[0], "detail-ins", &|ui| {
                if tx.is_coinbase {
                    meta(ui, cx.say("detail.new_coins"), t);
                }
                for input in &tx.ins {
                    coin_line(ui, cx, input.value, &input.from, cx.say("detail.older_coin"));
                }
            });
            section(&mut columns[1], &cx.fill("detail.outputs", &[&tx.outputs.len().to_string()]));
            list(&mut columns[1], "detail-outs", &|ui| {
                for output in &tx.outputs {
                    let other = match (&output.note, &output.lock) {
                        (Some(note), _) => note.clone(),
                        (None, Some(lock)) => lock.clone(),
                        (None, None) => cx.say("detail.under_lock").to_string(),
                    };
                    coin_line(ui, cx, Some(output.value), &output.to, &other);
                }
            });
        });
    }

    fn name_row(&mut self, ui: &mut Ui, cx: &Cx) {
        let id = hex(&self.item.id);
        if !self.named {
            self.name = label_of(cx, &self.item.id).unwrap_or_default().to_string();
            self.named = true;
        }
        meta(ui, cx.say("detail.name_hint"), cx.t);
        ui.horizontal(|ui| {
            let width = (ui.available_width() - 100.0).max(120.0);
            field(ui, &mut self.name, "", width);
            if button(ui, cx.t, cx.say("detail.name_save"), Kind::Plain).clicked() {
                cx.send(Command::Own(Personal::Wallet(WalletOrder::Label { of: id, name: self.name.clone() })));
            }
        });
    }

    /// True when it is finished.
    pub fn show(&mut self, ui: &mut Ui, cx: &mut Cx) -> bool {
        let t = cx.t;
        sheet_head(ui, cx.say(kind_key(self.item.kind)));
        if self.item.kind == ActivityKind::Joined {
            explain::short(ui, t, cx.say("detail.joined_about"));
        }
        if let Some(left) = blocks_to_ripe(&self.item, cx.snap.node.height) {
            explain::short(ui, t, &cx.fill("detail.ripe_about", &[&format::number(left.into())]));
        }
        ui.add_space(tokens::GAP);
        let mut done = self.facts(ui, cx);
        ui.add_space(tokens::GAP);
        self.coins(ui, cx);
        ui.add_space(tokens::GAP);
        self.name_row(ui, cx);
        ui.add_space(tokens::GAP);
        ui.horizontal(|ui| {
            let waiting = self.item.height.is_none() && self.item.kind == ActivityKind::Sent;
            if waiting && button(ui, t, cx.say("home.speed_up"), Kind::Main).clicked() {
                *cx.slip = Some(cx.slip_for(speed_up_slip(cx, &self.item)));
                done = true;
            }
            done |= button(ui, t, cx.say("common.close"), Kind::Plain).clicked();
        });
        done
    }
}

/// The slip before a waiting payment goes again with a higher fee, as Home's had it.
fn speed_up_slip(cx: &Cx, item: &Activity) -> Slipping {
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
        order: vec![Command::Lock(LockOrder::SpeedUp { id: hex(&item.id) })],
    }
}

/// What each kind does to the balance, on the same two questions for every kind.
pub fn kinds_explained(ui: &mut Ui, cx: &Cx) {
    let t = cx.t;
    let kinds = [ActivityKind::Received, ActivityKind::Sent, ActivityKind::Mined, ActivityKind::Joined, ActivityKind::ToSelf];
    let values: Vec<[&str; 2]> = kinds
        .iter()
        .map(|kind| match kind {
            ActivityKind::Received => [cx.say("explain.up"), cx.say("explain.received")],
            ActivityKind::Sent => [cx.say("explain.down"), cx.say("explain.sent")],
            ActivityKind::Mined => [cx.say("explain.up_later"), cx.say("explain.mined")],
            ActivityKind::Joined => [cx.say("explain.fee_only"), cx.say("explain.joined")],
            ActivityKind::ToSelf => [cx.say("explain.fee_only"), cx.say("explain.to_self")],
        })
        .collect();
    let choices: Vec<Choice> = kinds.iter().zip(&values).map(|(kind, values)| Choice { name: cx.say(kind_key(*kind)), values }).collect();
    explain::compare(ui, t, "history-kinds", &[cx.say("explain.balance"), cx.say("explain.why")], &choices);
    ui.add_space(tokens::GAP);
    meta(ui, &cx.fill("explain.settled", &[&SETTLED.to_string()]), t);
}
