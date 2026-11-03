//! Everything written down: the latest blocks and who found each first, then a search for a
//! block, payment or address above the list, and notes written into blocks last.
use super::{Cx, Page, Slipping};
use super::records_detail;
use crate::core::{short_hex, unix_ms, Command, Explored, Personal};
use crate::ui::chain::{self, Look, Strip};
use crate::ui::format;
use crate::ui::theme::desktop_reduces_motion;
use crate::ui::tokens;
use crate::ui::widgets::{
    self, button, button_on, cell, field, header_cell, meta, mono, section, Columns, Kind,
};
use crate::ui::Icon;
use egui::Ui;

const WEIGHTS: [f32; 5] = [0.7, 1.1, 1.3, 1.3, 0.7];
/// An expert's list has the block's hash too, as an explorer's has.
const EXPERT_WEIGHTS: [f32; 6] = [0.7, 1.2, 1.1, 0.8, 1.0, 0.6];
/// The blocks on this page, as large as on Home.
const BLOCK_SIZE: f32 = 70.0;

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

    /// The latest blocks as boxes, with who found each, as on Home.
    fn strip(ui: &mut Ui, cx: &Cx) {
        let t = cx.t;
        section(ui, cx.say("records.recent"));
        let chain_view = &cx.snap.node.chain;
        if !chain::has_blocks(chain_view) {
            return;
        }
        ui.add_space(4.0);
        let ticker = cx.ticker().to_string();
        let still = cx.settings.reduce_motion || desktop_reduces_motion();
        let look = Look { t, voice: cx.voice(), still, now_ms: unix_ms(), ticker: &ticker };
        chain::block_strip(ui, &look, &Strip::of(chain_view).size(BLOCK_SIZE));
        ui.add_space(tokens::GAP);
    }

    /// Who found a block, when it is among the latest the node names.
    fn finder(cx: &Cx, id: &olc_core::Hash) -> String {
        let latest = &cx.snap.node.chain.latest;
        let miner = latest.iter().find(|mined| mined.id == *id).map(|mined| &mined.miner);
        chain::miner_name(cx.voice(), miner.unwrap_or(&crate::core::Miner::Unknown))
    }

    fn blocks(&mut self, ui: &mut Ui, cx: &Cx) {
        let t = cx.t;
        if cx.snap.node.blocks.is_empty() {
            meta(ui, cx.say("records.no_blocks"), t);
            return;
        }
        let now = unix_ms();
        let expert = cx.expert();
        let columns = Columns::new(ui, if expert { &EXPERT_WEIGHTS[..] } else { &WEIGHTS[..] });
        let keys: &[&str] = if expert {
            &["records.number", "records.when", "records.miner", "records.payments", "pro.block_id"]
        } else {
            &["records.number", "records.when", "records.miner", "records.payments"]
        };
        columns.row(ui, |ui, index| {
            if let Some(key) = keys.get(index) {
                header_cell(ui, t, cx.say(key));
            }
        });
        widgets::divider(ui, t);
        for block in &cx.snap.node.blocks {
            columns.row(ui, |ui, index| match index {
                0 => cell(ui, format::number(block.height.into())),
                1 => cell(ui, format::when(cx.voice(), now, block.time_ms)),
                2 => cell(ui, Self::finder(cx, &block.id)),
                3 => cell(ui, block.payments.to_string()),
                4 if expert => cell(ui, mono(short_hex(&block.id))),
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
        Self::strip(ui, cx);
        self.search(ui, cx);
        // A block or payment opened from the history shows here too.
        let opened = !matches!(cx.snap.node.explored, Explored::Nothing);
        if self.asked || opened {
            let ticker = cx.ticker().to_string();
            if let Some(next) = records_detail::result(ui, cx, &ticker) {
                self.search = next.clone();
                look_up(cx, next);
                self.asked = true;
            }
        }
        ui.add_space(tokens::GAP);
        self.blocks(ui, cx);
        if cx.expert() {
            ui.add_space(tokens::GAP_WIDE);
            records_detail::waiting(ui, cx);
        }
        ui.add_space(tokens::GAP_WIDE);
        self.write(ui, cx);
        ui.add_space(tokens::GAP_WIDE);
        Self::notes(ui, cx);
    }
}
