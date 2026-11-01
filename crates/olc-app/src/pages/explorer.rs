//! The chain as this node holds it: blocks, payments, addresses and the notes people left.
use super::{Cx, Page};
use crate::core::{short_hex, Command, Explored, Personal, TxRow};
use crate::ui::format;
use crate::ui::tokens;
use crate::ui::widgets::{
    self, button, button_on, card, cell, field, header_cell, meta, mono, section, stats, Columns,
    Kind,
};
use crate::ui::Icon;
use egui::Ui;

const WEIGHTS: [f32; 4] = [0.8, 2.0, 0.7, 0.8];

#[derive(Default)]
pub struct Explorer {
    search: String,
    note: String,
    /// Results wait for a first question, so an empty page does not say "not found".
    asked: bool,
}

fn look_up(cx: &Cx, text: String) {
    cx.send(Command::Own(Personal::Explore(text)));
}

fn full_hex(id: &[u8]) -> String {
    id.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn show_tx(ui: &mut Ui, cx: &Cx, tx: &TxRow, ticker: &str) {
    let t = cx.t;
    let kind = match tx.is_coinbase {
        true => cx.say("explorer.coinbase").to_string(),
        false => cx.fill("explorer.inputs", &[&tx.inputs.to_string()]),
    };
    ui.label(mono(full_hex(&tx.id)));
    meta(ui, &kind, t);
    for output in &tx.outputs {
        match &output.note {
            Some(note) => {
                ui.label(cx.fill("explorer.note_line", &[note]));
            }
            None => {
                let text = format!("{}  {}", format::coins(output.value, ticker), output.to);
                ui.label(mono(text));
            }
        }
    }
    ui.add_space(4.0);
}

fn result(ui: &mut Ui, cx: &Cx, ticker: &str) {
    let t = cx.t;
    match &cx.snap.node.explored {
        Explored::Nothing => {
            meta(ui, cx.say("explorer.nothing"), t);
        }
        Explored::Pruned { height, oldest } => {
            let words = [format::number((*height).into()), format::number((*oldest).into())];
            meta(ui, &cx.fill("explorer.pruned", &[&words[0], &words[1]]), t);
        }
        Explored::Block(block) => {
            let height = format::number(block.row.height.into());
            section(ui, &cx.fill("explorer.block", &[&height]));
            ui.label(mono(full_hex(&block.row.id)));
            meta(ui, &cx.fill("explorer.previous", &[&short_hex(&block.previous)]), t);
            for tx in &block.txs {
                show_tx(ui, cx, tx, ticker);
            }
        }
        Explored::Tx { height, tx } => {
            let height = format::number((*height).into());
            section(ui, &cx.fill("explorer.block", &[&height]));
            show_tx(ui, cx, tx, ticker);
        }
        Explored::Address { address, balance, coins } => {
            section(ui, address);
            let items = [
                (cx.say("wallet.balance"), format::coins(*balance, ticker)),
                (cx.say("explorer.coins"), format::number((*coins as u64).into())),
            ];
            stats(ui, t, &items);
        }
    }
}

impl Explorer {
    fn search(&mut self, ui: &mut Ui, cx: &Cx) {
        let t = cx.t;
        ui.horizontal(|ui| {
            field(ui, &mut self.search, cx.say("explorer.search"), 460.0);
            let ready = !self.search.trim().is_empty();
            if button_on(ui, t, cx.say("explorer.find"), Kind::Main, ready).clicked() {
                look_up(cx, self.search.clone());
                self.asked = true;
            }
        });
    }

    fn write(&mut self, ui: &mut Ui, cx: &Cx) {
        let t = cx.t;
        section(ui, cx.say("explorer.write"));
        ui.horizontal(|ui| {
            field(ui, &mut self.note, cx.say("explorer.write_hint"), 460.0);
            let size = self.note.len();
            let fits = size > 0 && size <= olc_node::MOST_DATA;
            if button_on(ui, t, cx.say("explorer.send"), Kind::Plain, fits).clicked() {
                cx.send(Command::Own(Personal::WriteNote(std::mem::take(&mut self.note))));
            }
        });
        let most = olc_node::MOST_DATA.to_string();
        meta(ui, &cx.fill("explorer.write_note", &[&most]), t);
    }

    fn blocks(&mut self, ui: &mut Ui, cx: &Cx) {
        let t = cx.t;
        section(ui, cx.say("explorer.recent"));
        let columns = Columns::new(ui, &WEIGHTS);
        columns.row(ui, |ui, index| {
            let keys = ["column.height", "explorer.id", "explorer.payments"];
            if let Some(key) = keys.get(index) {
                header_cell(ui, t, cx.say(key));
            }
        });
        for block in &cx.snap.node.blocks {
            columns.row(ui, |ui, index| match index {
                0 => cell(ui, format::number(block.height.into())),
                1 => cell(ui, mono(short_hex(&block.id))),
                2 => cell(ui, block.payments.to_string()),
                _ => {
                    if button(ui, t, cx.say("explorer.open"), Kind::Quiet).clicked() {
                        self.search = block.height.to_string();
                        look_up(cx, block.height.to_string());
                        self.asked = true;
                    }
                }
            });
        }
    }

    fn notes(ui: &mut Ui, cx: &Cx) {
        let t = cx.t;
        section(ui, cx.say("explorer.notes"));
        if cx.snap.node.notes.is_empty() {
            meta(ui, cx.say("explorer.no_notes"), t);
        }
        for note in &cx.snap.node.notes {
            let height = format::number(note.height.into());
            ui.label(cx.fill("explorer.note_at", &[&height, &note.text]));
        }
    }
}

impl Page for Explorer {
    fn title(&self) -> &'static str {
        "page.explorer"
    }

    fn icon(&self) -> Icon {
        Icon::Explorer
    }

    fn show(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let t = cx.t;
        let ticker = cx.snap.node.coin.as_ref().map_or("", |coin| coin.ticker.as_str()).to_string();
        widgets::title(ui, cx.say("page.explorer"));
        card(ui, t, |ui| {
            self.search(ui, cx);
            if self.asked {
                result(ui, cx, &ticker);
            }
        });
        ui.add_space(tokens::GAP);
        card(ui, t, |ui| self.write(ui, cx));
        ui.add_space(tokens::GAP);
        card(ui, t, |ui| Self::notes(ui, cx));
        ui.add_space(tokens::GAP);
        card(ui, t, |ui| self.blocks(ui, cx));
    }
}
