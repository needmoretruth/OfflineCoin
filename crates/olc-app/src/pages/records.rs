//! Everything written down: blocks, payments, addresses and the notes people left.
use super::{Cx, Page, Slipping};
use crate::core::{short_hex, unix_ms, Command, Explored, Personal, TxRow};
use crate::ui::format;
use crate::ui::tokens;
use crate::ui::widgets::{
    self, button, button_on, cell, field, header_cell, meta, mono, section, Columns, Kind,
};
use crate::ui::Icon;
use egui::Ui;

const WEIGHTS: [f32; 4] = [0.8, 1.2, 1.5, 0.8];

#[derive(Default)]
pub struct Records {
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
        true => cx.say("records.mining_reward").to_string(),
        false => cx.fill("records.inputs", &[&tx.inputs.to_string()]),
    };
    ui.label(mono(full_hex(&tx.id)));
    meta(ui, &kind, t);
    for output in &tx.outputs {
        match &output.note {
            Some(note) => {
                ui.label(cx.fill("records.note_line", &[note]));
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
    ui.add_space(tokens::GAP);
    match &cx.snap.node.explored {
        Explored::Nothing => {
            meta(ui, cx.say("records.nothing"), t);
        }
        Explored::Pruned { height, oldest } => {
            let words = [format::number((*height).into()), format::number((*oldest).into())];
            meta(ui, &cx.fill("records.pruned", &[&words[0], &words[1]]), t);
        }
        Explored::Block(block) => {
            let height = format::number(block.row.height.into());
            section(ui, &cx.fill("records.block", &[&height]));
            ui.label(mono(full_hex(&block.row.id)));
            meta(ui, &cx.fill("records.previous", &[&short_hex(&block.previous)]), t);
            for tx in &block.txs {
                show_tx(ui, cx, tx, ticker);
            }
        }
        Explored::Tx { height, tx } => {
            let height = format::number((*height).into());
            section(ui, &cx.fill("records.block", &[&height]));
            show_tx(ui, cx, tx, ticker);
        }
        Explored::Address { address, balance, coins } => {
            ui.label(mono(address.clone()));
            let value = format::coins(*balance, ticker);
            meta(ui, &cx.fill("records.address_line", &[&value, &coins.to_string()]), t);
        }
    }
}

impl Records {
    fn search(&mut self, ui: &mut Ui, cx: &Cx) {
        let t = cx.t;
        ui.horizontal(|ui| {
            let hint = cx.say("records.search");
            let width = (ui.available_width() - 110.0).min(460.0);
            let response = field(ui, &mut self.search, hint, width);
            let enter = response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
            let ready = !self.search.trim().is_empty();
            let find = button_on(ui, t, cx.say("records.find"), Kind::Plain, ready).clicked();
            if ready && (find || enter) {
                look_up(cx, self.search.clone());
                self.asked = true;
            }
        });
    }

    fn write(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let t = cx.t;
        section(ui, cx.say("records.write"));
        let mut slip = None;
        ui.horizontal(|ui| {
            let width = (ui.available_width() - 110.0).min(460.0);
            field(ui, &mut self.note, cx.say("records.write_hint"), width);
            let size = self.note.len();
            let fits = size > 0 && size <= olc_node::MOST_DATA;
            if button_on(ui, t, cx.say("records.write_go"), Kind::Plain, fits).clicked() {
                let note = std::mem::take(&mut self.note);
                slip = Some(cx.slip_for(Slipping {
                    what: cx.say("records.write"),
                    rows: vec![(cx.say("records.note"), note.clone())],
                    warning: cx.say("records.write_warning"),
                    sign: cx.say("records.write_go"),
                    danger: false,
                    order: vec![Command::Own(Personal::WriteNote(note))],
                }));
            }
        });
        if slip.is_some() {
            *cx.slip = slip;
        }
        let most = olc_node::MOST_DATA.to_string();
        meta(ui, &cx.fill("records.write_about", &[&most]), t);
    }

    fn blocks(&mut self, ui: &mut Ui, cx: &Cx) {
        let t = cx.t;
        section(ui, cx.say("records.recent"));
        if cx.snap.node.blocks.is_empty() {
            meta(ui, cx.say("records.no_blocks"), t);
            return;
        }
        let now = unix_ms();
        let columns = Columns::new(ui, &WEIGHTS);
        columns.row(ui, |ui, index| {
            let keys = ["records.number", "records.when", "records.payments"];
            if let Some(key) = keys.get(index) {
                header_cell(ui, t, cx.say(key));
            }
        });
        widgets::divider(ui, t);
        for block in &cx.snap.node.blocks {
            columns.row(ui, |ui, index| match index {
                0 => cell(ui, format::number(block.height.into())),
                1 => cell(ui, format::ago(cx.voice(), now, block.time_ms)),
                2 if cx.expert() => cell(ui, mono(short_hex(&block.id))),
                2 => cell(ui, block.payments.to_string()),
                _ => {
                    if button(ui, t, cx.say("records.open"), Kind::Quiet).clicked() {
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
        section(ui, cx.say("records.notes"));
        if cx.snap.node.notes.is_empty() {
            meta(ui, cx.say("records.no_notes"), t);
        }
        for note in &cx.snap.node.notes {
            let height = format::number(note.height.into());
            ui.label(cx.fill("records.note_at", &[&height, &note.text]));
        }
    }
}

impl Page for Records {
    fn title(&self) -> &'static str {
        "page.records"
    }

    fn icon(&self) -> Icon {
        Icon::Explorer
    }

    fn show(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let t = cx.t;
        widgets::title(ui, cx.say("page.records"));
        if cx.snap.node.light.is_some() {
            let go = cx.say("records.keep_more");
            if widgets::empty(ui, t, cx.say("records.light"), cx.say("records.light_body"), Some(go)) {
                *cx.go = Some("page.settings");
            }
            return;
        }
        self.search(ui, cx);
        if self.asked {
            let ticker = cx.ticker().to_string();
            result(ui, cx, &ticker);
        }
        ui.add_space(tokens::GAP_WIDE);
        self.write(ui, cx);
        ui.add_space(tokens::GAP_WIDE);
        Self::notes(ui, cx);
        ui.add_space(tokens::GAP_WIDE);
        self.blocks(ui, cx);
    }
}
