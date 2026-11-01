//! A pool on this app's node, over the nodes and boards a person ticks.
use super::{Cx, Page};
use crate::core::Command;
use crate::ui::format;
use crate::ui::tokens;
use crate::ui::widgets::{
    self, button, button_on, card, cell, header_cell, meta, number_field, section, stats,
    switch_row, Columns, Kind,
};
use crate::ui::Icon;
use egui::Ui;
use olc_control::BoardId;
use std::collections::BTreeSet;

/// Shares a block is worth when nobody chose: often enough to see, rare enough to check.
const SHARES_PER_BLOCK: u64 = 64;

#[derive(Default)]
pub struct PoolPage {
    share_difficulty: u64,
    nodes: BTreeSet<u32>,
    boards: BTreeSet<BoardId>,
}

impl PoolPage {
    fn form(&mut self, ui: &mut Ui, cx: &Cx) {
        let t = cx.t;
        section(ui, cx.say("pool.workers"));
        for view in &cx.snap.local {
            let mut on = self.nodes.contains(&view.spec.id);
            if switch_row(ui, t, &mut on, &view.spec.name) {
                let _ = if on { self.nodes.insert(view.spec.id) } else { self.nodes.remove(&view.spec.id) };
            }
        }
        for board in &cx.snap.boards {
            let id = board.seen.id;
            let mut on = self.boards.contains(&id);
            if switch_row(ui, t, &mut on, &board.seen.beacon.name) {
                let _ = if on { self.boards.insert(id) } else { self.boards.remove(&id) };
            }
        }
        if cx.snap.local.is_empty() && cx.snap.boards.is_empty() {
            meta(ui, cx.say("pool.none"), t);
        }
        if self.share_difficulty == 0 {
            self.share_difficulty = (cx.snap.node.difficulty / SHARES_PER_BLOCK).max(1);
        }
        ui.horizontal(|ui| {
            ui.label(cx.say("pool.share_difficulty"));
            number_field(ui, &mut self.share_difficulty, 120.0);
        });
        let ready = !self.nodes.is_empty() || !self.boards.is_empty();
        if button_on(ui, t, cx.say("pool.start"), Kind::Main, ready).clicked() {
            cx.send(Command::StartPool {
                share_difficulty: self.share_difficulty,
                nodes: self.nodes.iter().copied().collect(),
                boards: self.boards.iter().copied().collect(),
            });
        }
        meta(ui, cx.say("pool.note"), t);
    }
}

fn running(ui: &mut Ui, cx: &Cx) {
    let t = cx.t;
    let pool = &cx.snap.pool;
    let ticker = cx.snap.node.coin.as_ref().map_or("", |coin| coin.ticker.as_str());
    stats(
        ui,
        t,
        &[
            (cx.say("pool.share_difficulty"), format::number(pool.share_difficulty.into())),
            (cx.say("pool.blocks"), format::number(pool.blocks.into())),
            (cx.say("pool.lost"), format::number(pool.lost.into())),
        ],
    );
    let columns = Columns::new(ui, &[1.6, 1.0, 1.0, 1.4]);
    columns.row(ui, |ui, index| {
        let keys = ["pool.worker", "pool.shares", "pool.rejected", "pool.paid"];
        header_cell(ui, t, cx.say(keys[index]));
    });
    for row in &pool.rows {
        columns.row(ui, |ui, index| match index {
            0 => cell(ui, row.name.clone()),
            1 => cell(ui, format::number(row.shares.into())),
            2 => cell(ui, format::number(row.rejected.into())),
            _ => cell(ui, format::coins(row.paid, ticker)),
        });
    }
    ui.add_space(tokens::GAP);
    if button(ui, t, cx.say("pool.stop"), Kind::Plain).clicked() {
        cx.send(Command::StopPool);
    }
}

impl Page for PoolPage {
    fn title(&self) -> &'static str {
        "page.pool"
    }

    fn icon(&self) -> Icon {
        Icon::Pool
    }

    fn show(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let t = cx.t;
        widgets::title(ui, cx.say("page.pool"));
        card(ui, t, |ui| match cx.snap.pool.running {
            true => running(ui, cx),
            false => self.form(ui, cx),
        });
    }
}
