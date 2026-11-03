//! What the explorer opened: a block with its time, miner, size and fees and the blocks either
//! side; a payment with the coins in and out; an address with its payments; and, for an expert,
//! the payments waiting for a block.
use super::Cx;
use crate::core::{short_hex, unix_ms, Explored, TxRow};
use crate::ui::format;
use crate::ui::tokens;
use crate::ui::widgets::{self, button, cell, header_cell, meta, mono, section, tag, Columns, Kind};
use egui::{Grid, Label, Ui};

const MOVE_WEIGHTS: [f32; 4] = [0.7, 1.4, 1.2, 0.6];
const WAITING_WEIGHTS: [f32; 5] = [1.2, 0.9, 0.7, 0.8, 0.9];

fn full_hex(id: &[u8]) -> String {
    id.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn is_mine(cx: &Cx, address: &str) -> bool {
    !address.is_empty() && cx.snap.node.keys.iter().any(|key| key.address == address)
}

/// An address, its name when it has one, and a tag when it is this wallet's.
fn address_cell(ui: &mut Ui, cx: &Cx, address: &str) {
    ui.add(Label::new(mono(format::short_address(address))).truncate());
    if is_mine(cx, address) {
        tag(ui, cx.say("detail.mine"), cx.t.good, cx.t);
    }
    if let Some(name) = cx.snap.node.wallet.labels.get(address) {
        meta(ui, name, cx.t);
    }
}

fn fact(ui: &mut Ui, cx: &Cx, key: &str, value: impl Into<egui::WidgetText>) {
    meta(ui, cx.say(key), cx.t);
    ui.label(value);
    ui.end_row();
}

/// One payment: the coins it spent, the coins it made, its fee and size.
fn show_tx(ui: &mut Ui, cx: &Cx, tx: &TxRow, ticker: &str) {
    let t = cx.t;
    ui.add(Label::new(mono(full_hex(&tx.id))).truncate());
    let mut line = match tx.is_coinbase {
        true => cx.say("records.mining_reward").to_string(),
        false => cx.fill("records.inputs", &[&tx.inputs.to_string()]),
    };
    if let Some(fee) = tx.fee {
        line = format!("{line} · {}", cx.fill("records.fee_line", &[&format::coins(fee, ticker)]));
    }
    line = format!("{line} · {}", cx.fill("detail.bytes", &[&format::number(tx.size.into())]));
    meta(ui, &line, t);
    for input in &tx.ins {
        ui.horizontal(|ui| {
            let value = input.value.map_or_else(|| "?".to_string(), |value| format::coins(value, ticker));
            ui.label(format!("− {value}"));
            if input.from.is_empty() {
                meta(ui, cx.say("detail.older_coin"), t);
            } else {
                address_cell(ui, cx, &input.from);
            }
        });
    }
    for output in &tx.outputs {
        ui.horizontal(|ui| match (&output.note, &output.lock) {
            (Some(note), _) => {
                ui.label(cx.fill("records.note_line", &[note]));
            }
            (None, Some(words)) => {
                ui.label(format!("+ {}", format::coins(output.value, ticker)));
                ui.add(Label::new(mono(words.clone())).truncate());
            }
            (None, None) => {
                ui.label(format!("+ {}", format::coins(output.value, ticker)));
                address_cell(ui, cx, &output.to);
            }
        });
    }
    ui.add_space(tokens::GAP);
}

/// What the last question found. Returns what to look up next when a link was clicked.
pub fn result(ui: &mut Ui, cx: &Cx, ticker: &str) -> Option<String> {
    let t = cx.t;
    let mut next = None;
    ui.add_space(tokens::GAP);
    let tip = cx.snap.node.height;
    match &cx.snap.node.explored {
        Explored::Nothing => {
            meta(ui, cx.say("records.nothing"), t);
        }
        Explored::Pruned { height, oldest } => {
            let words = [format::number((*height).into()), format::number((*oldest).into())];
            meta(ui, &cx.fill("records.pruned", &[&words[0], &words[1]]), t);
        }
        Explored::Block(block) => {
            let height = block.row.height;
            section(ui, &cx.fill("records.block", &[&format::number(height.into())]));
            ui.horizontal(|ui| {
                if height > 0 && button(ui, t, cx.say("records.go_previous"), Kind::Quiet).clicked() {
                    next = Some((height - 1).to_string());
                }
                if block.next.is_some() && button(ui, t, cx.say("records.go_next"), Kind::Quiet).clicked() {
                    next = Some((height + 1).to_string());
                }
            });
            Grid::new("block-facts").num_columns(2).min_col_width(120.0).spacing([tokens::GAP_WIDE, 4.0]).show(ui, |ui| {
                let when = format!("{} · {}", format::moment(block.row.time_ms), format::ago(cx.voice(), unix_ms(), block.row.time_ms));
                fact(ui, cx, "records.block_time", when);
                let confirmations = tip.saturating_sub(height) + 1;
                fact(ui, cx, "records.confirmations", format::number(confirmations.into()));
                meta(ui, cx.say("records.miner"), t);
                ui.horizontal(|ui| address_cell(ui, cx, &block.miner));
                ui.end_row();
                fact(ui, cx, "detail.size", cx.fill("detail.bytes", &[&format::number(block.size.into())]));
                fact(ui, cx, "records.fees", format::coins(block.fees, ticker));
                fact(ui, cx, "records.payments", block.txs.len().to_string());
                if cx.expert() {
                    fact(ui, cx, "pro.block_id", mono(full_hex(&block.row.id)));
                }
            });
            ui.add_space(tokens::GAP);
            for tx in &block.txs {
                show_tx(ui, cx, tx, ticker);
            }
        }
        Explored::Tx { height, tx } => {
            ui.horizontal(|ui| {
                section(ui, &cx.fill("records.block", &[&format::number((*height).into())]));
                if button(ui, t, cx.say("records.open"), Kind::Quiet).clicked() {
                    next = Some(height.to_string());
                }
            });
            let confirmations = tip.saturating_sub(*height) + 1;
            meta(ui, &cx.fill("records.confirmed_by", &[&format::number(confirmations.into())]), t);
            show_tx(ui, cx, tx, ticker);
        }
        Explored::Address { address, balance, coins, moves, indexed_from } => {
            ui.add(Label::new(mono(address.clone())).wrap());
            if let Some(name) = cx.snap.node.wallet.labels.get(address) {
                ui.label(name.as_str());
            }
            let value = format::coins(*balance, ticker);
            meta(ui, &cx.fill("records.address_line", &[&value, &coins.to_string()]), t);
            ui.add_space(tokens::GAP);
            section(ui, cx.say("records.address_moves"));
            next = moves_table(ui, cx, moves, ticker).or(next);
            meta(ui, &cx.fill("records.indexed_from", &[&format::number((*indexed_from).into())]), t);
        }
    }
    next
}

fn moves_table(ui: &mut Ui, cx: &Cx, moves: &[crate::core::AddressMove], ticker: &str) -> Option<String> {
    let t = cx.t;
    if moves.is_empty() {
        meta(ui, cx.say("records.no_moves"), t);
        return None;
    }
    let mut next = None;
    let now = unix_ms();
    let columns = Columns::new(ui, &MOVE_WEIGHTS);
    columns.row(ui, |ui, index| {
        let keys = ["records.number", "records.when", "records.change"];
        if let Some(key) = keys.get(index) {
            header_cell(ui, t, cx.say(key));
        }
    });
    widgets::divider(ui, t);
    for entry in moves {
        let change = i128::from(entry.received) - i128::from(entry.spent);
        let sign = match change { ..0 => "−", 0 => "", _ => "+" };
        let amount = format!("{sign}{}", format::coins(u64::try_from(change.unsigned_abs()).unwrap_or(u64::MAX), ticker));
        columns.row(ui, |ui, index| match index {
            0 => cell(ui, format::number(entry.height.into())),
            1 => cell(ui, format::when(cx.voice(), now, entry.time_ms)),
            2 => cell(ui, amount.clone()),
            _ => {
                if button(ui, t, cx.say("records.open"), Kind::Quiet).clicked() {
                    next = Some(full_hex(&entry.id));
                }
            }
        });
    }
    next
}

/// The payments waiting for a block, best fee first, for an expert.
pub fn waiting(ui: &mut Ui, cx: &Cx) {
    let t = cx.t;
    let pool = &cx.snap.node.wallet.pool;
    section(ui, &cx.fill("pro.waiting_title", &[&pool.len().to_string()]));
    if pool.is_empty() {
        meta(ui, cx.say("pro.waiting_none"), t);
        return;
    }
    let ticker = cx.ticker();
    let now = unix_ms();
    let columns = Columns::new(ui, &WAITING_WEIGHTS);
    columns.row(ui, |ui, index| {
        let keys = ["pro.waiting_id", "detail.fee", "pro.waiting_rate", "detail.size", "pro.waiting_age"];
        if let Some(key) = keys.get(index) {
            header_cell(ui, t, cx.say(key));
        }
    });
    widgets::divider(ui, t);
    for row in pool.iter().take(50) {
        columns.row(ui, |ui, index| match index {
            0 => {
                ui.add(Label::new(mono(short_hex(&row.id))).truncate());
                if row.mine {
                    tag(ui, cx.say("detail.mine"), t.good, t);
                }
            }
            1 => cell(ui, format::coins(row.fee, ticker)),
            2 => cell(ui, cx.fill("send.rate", &[&(row.fee / row.size.max(1)).to_string()])),
            3 => cell(ui, format::number(row.size.into())),
            _ => cell(ui, format::ago(cx.voice(), now, row.seen_ms)),
        });
    }
}
