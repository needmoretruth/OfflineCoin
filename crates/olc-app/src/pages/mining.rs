//! Where miners are managed. First every miner this computer runs, this app's own and the ones
//! added, with this app's speed control and the line of their speed together; beside them the race
//! against everyone and how blocks are paced. Then boards as add-ons, each added miner in detail,
//! and mining together. Home keeps the short card; this page holds the controls.
use super::boards::state_colour;
use super::local::Local;
use super::pool::PoolTab;
use super::speed::Speed;
use super::{miners, pacing, Cx, Page};
use crate::core::unix_ms;
use crate::ui::chain::{self, Look};
use crate::ui::explain;
use crate::ui::theme::desktop_reduces_motion;
use crate::ui::tokens;
use crate::ui::widgets::{self, button, button_on, card, dot, meta, section, Kind};
use crate::ui::{format, Icon};
use egui::{vec2, Align, Layout, RichText, Ui};

/// How wide the miners take of the first row; the race and the pacing take the rest.
const MINERS_SHARE: f32 = 0.58;
const LINE_HEIGHT: f32 = 130.0;

#[derive(Default)]
pub struct Mining {
    speed: Speed,
    local: Local,
    pool: PoolTab,
    /// The id the next added miner gets.
    next_miner: u32,
}

fn details_labels(cx: &Cx) -> (&'static str, &'static str) {
    (cx.say("explain.details"), cx.say("explain.hide"))
}

/// Every miner's share of the latest blocks and its speed, most blocks first.
fn race(ui: &mut Ui, cx: &Cx, still: bool) {
    let t = cx.t;
    card(ui, t, |ui| {
        section(ui, cx.say("mining.race"));
        explain::short(ui, t, cx.say("mining.race_about"));
        ui.add_space(tokens::GAP);
        let ticker = cx.ticker().to_string();
        let look = Look { t, voice: cx.voice(), still, now_ms: unix_ms(), ticker: &ticker };
        chain::race_bars(ui, &look, &cx.snap.node.chain.shares);
        ui.add_space(tokens::GAP);
        chain::next_block_bar(ui, &look, &cx.snap.node.chain);
        explain::details(ui, t, "mining-race", details_labels(cx), |ui| {
            explain::analogy(ui, t, cx.say("mining.race_analogy"));
            ui.add_space(tokens::GAP);
            ui.label(cx.say("mining.race_more"));
        });
    });
}

impl Mining {
    /// Every miner here in one table, this app's speed control under it, and their speed together.
    fn miners(&mut self, ui: &mut Ui, cx: &mut Cx, still: bool) {
        let t = *cx.t;
        card(ui, &t, |ui| {
            ui.horizontal(|ui| {
                section(ui, cx.say("mining.computer"));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    // A new miner starts at a share of the full speed, known once it is measured.
                    let ready = cx.snap.full_rate > 0;
                    if button_on(ui, &t, cx.say("mining.add"), Kind::Plain, ready).clicked() {
                        miners::add(cx, &mut self.next_miner);
                    }
                });
            });
            explain::short(ui, &t, cx.say("mining.more_about"));
            ui.add_space(4.0);
            miners::table(ui, cx);
            ui.add_space(tokens::GAP_WIDE);
            self.speed.show(ui, cx, "mining");
            ui.add_space(tokens::GAP_WIDE);
            let ticker = cx.ticker().to_string();
            let look = Look { t: &t, voice: cx.voice(), still, now_ms: unix_ms(), ticker: &ticker };
            chain::hashrate_line(ui, &look, &cx.snap.node.chain, LINE_HEIGHT);
        });
    }

    fn first_row(&mut self, ui: &mut Ui, cx: &mut Cx, still: bool) {
        if ui.available_width() < tokens::COMPACT {
            self.miners(ui, cx, still);
            ui.add_space(tokens::GAP_WIDE);
            race(ui, cx, still);
            ui.add_space(tokens::GAP_WIDE);
            pacing::pacing_card(ui, cx, details_labels(cx));
            return;
        }
        let total = ui.available_width();
        let left = ((total - tokens::GAP_WIDE) * MINERS_SHARE).round();
        let right = total - left - tokens::GAP_WIDE;
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(vec2(left, 0.0), Layout::top_down(Align::Min), |ui| {
                ui.set_width(left);
                self.miners(ui, cx, still);
            });
            ui.add_space(tokens::GAP_WIDE - ui.spacing().item_spacing.x);
            ui.allocate_ui_with_layout(vec2(right, 0.0), Layout::top_down(Align::Min), |ui| {
                ui.set_width(right);
                race(ui, cx, still);
                ui.add_space(tokens::GAP_WIDE);
                pacing::pacing_card(ui, cx, details_labels(cx));
            });
        });
    }

    /// Boards are an add-on: a line each, and the way to the Devices page.
    fn boards(ui: &mut Ui, cx: &mut Cx) {
        let t = cx.t;
        ui.horizontal(|ui| {
            section(ui, cx.say("mining.boards"));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if button(ui, t, cx.say("mining.open_devices"), Kind::Quiet).clicked() {
                    *cx.go = Some("page.boards");
                }
            });
        });
        explain::short(ui, t, cx.say("mining.boards_about"));
        ui.add_space(4.0);
        if cx.snap.boards.is_empty() {
            meta(ui, cx.say("mining.no_boards"), t);
            return;
        }
        for board in &cx.snap.boards {
            let beacon = &board.seen.beacon;
            ui.horizontal(|ui| {
                dot(ui, state_colour(t, board));
                ui.label(beacon.name.as_str());
                let line = match () {
                    () if !board.seen.online => cx.say("boards.gone").to_string(),
                    () if beacon.mining => format::rate(cx.voice(), beacon.hashrate.into()),
                    () => cx.say("boards.resting").to_string(),
                };
                ui.label(RichText::new(line).size(tokens::META).color(t.soft));
            });
        }
    }
}

impl Page for Mining {
    fn title(&self) -> &'static str {
        "page.mining"
    }

    fn icon(&self) -> Icon {
        Icon::Mining
    }

    fn daily(&self) -> bool {
        true
    }

    fn show(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let t = cx.t;
        let still = cx.settings.reduce_motion || desktop_reduces_motion();
        widgets::title(ui, cx.say("page.mining"));
        self.first_row(ui, cx, still);
        ui.add_space(tokens::GAP);
        // Each added miner's memory, disk, payments and the form with every number, one press away:
        // the table above already switches and removes them. It stands above the boards, so a board
        // that comes or goes does not move it.
        let labels = (cx.say("mining.more"), cx.say("explain.hide"));
        let local = &mut self.local;
        explain::details(ui, t, "mining-each", labels, |ui| local.show(ui, cx));
        ui.add_space(tokens::GAP_PAGE);
        Self::boards(ui, cx);
        ui.add_space(tokens::GAP_PAGE);
        section(ui, cx.say("mining.together"));
        ui.add_space(tokens::GAP);
        self.pool.show(ui, cx);
    }
}
