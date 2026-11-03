//! Where miners are managed. First every miner this computer runs, this app's own and the ones
//! added, with this app's speed control and the line of their speed together; beside them the race
//! against everyone and how blocks are paced. Then boards as add-ons, each added miner in detail,
//! and mining together. Home keeps the short card; this page holds the controls.
use super::boards::state_colour;
use super::local::Local;
use super::pool::PoolTab;
use super::speed::Speed;
use super::{miners, Cx, Page};
use crate::core::unix_ms;
use crate::ui::chain::{self, Look};
use crate::ui::explain;
use crate::ui::theme::desktop_reduces_motion;
use crate::ui::tokens::{self, Tokens};
use crate::ui::widgets::{self, button, button_on, card, dot, meta, section, Kind};
use crate::ui::{format, Icon};
use egui::{pos2, vec2, Align, CornerRadius, Label, Layout, Rect, RichText, Sense, Ui};

/// How wide the miners take of the first row; the race and the pacing take the rest.
const MINERS_SHARE: f32 = 0.58;
const STAT_TRACK: f32 = 6.0;
/// A figure in the pacing card needs this much room; fewer fit in a row on a narrow column.
const STAT_WIDTH: f32 = 150.0;
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

/// A name, a figure and a quieter line under it.
fn stat(ui: &mut Ui, t: &Tokens, name: &str, value: &str, under: &str) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        ui.add(Label::new(RichText::new(name).size(tokens::META).color(t.muted)).selectable(false));
        ui.add(Label::new(RichText::new(value).size(tokens::SECTION).family(crate::ui::fonts::strong())).truncate());
        if !under.is_empty() {
            ui.add(Label::new(RichText::new(under).size(tokens::META).color(t.soft)).wrap());
        }
    });
}

/// How far the blocks have come toward the next change of the target, as a filled track.
fn progress(ui: &mut Ui, t: &Tokens, part: f32, name: &str) {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(vec2(width, STAT_TRACK + 4.0), Sense::hover());
    let track = Rect::from_min_size(pos2(rect.left(), rect.top() + 2.0), vec2(width, STAT_TRACK));
    let radius = CornerRadius::same((STAT_TRACK / 2.0) as u8);
    ui.painter().rect_filled(track, radius, t.line_strong);
    let filled = Rect::from_min_size(track.min, vec2((width * part.clamp(0.0, 1.0)).max(STAT_TRACK), STAT_TRACK));
    ui.painter().rect_filled(filled, radius, t.soft);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ProgressIndicator, true, name));
}

/// One figure of the pacing card, some with a track of how far it has come.
enum Figure<'a> {
    Plain(&'a str, String, String),
    Track(&'a str, String, String, f32),
}

/// The figures in as many columns as the width holds, the rest on the next row.
fn figure_grid(ui: &mut Ui, t: &Tokens, figures: &[Figure]) {
    let gap = tokens::GAP_WIDE;
    let fits = ((ui.available_width() + gap) / (STAT_WIDTH + gap)).floor().max(1.0) as usize;
    let per_row = fits.min(figures.len()).max(1);
    let width = (ui.available_width() - gap * (per_row as f32 - 1.0)) / per_row as f32;
    for row in figures.chunks(per_row) {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for figure in row {
                ui.allocate_ui_with_layout(vec2(width, 0.0), Layout::top_down(Align::Min), |ui| {
                    ui.set_width(width);
                    match figure {
                        Figure::Plain(name, value, under) => stat(ui, t, name, value, under),
                        Figure::Track(name, value, under, part) => {
                            stat(ui, t, name, value, under);
                            ui.add_space(4.0);
                            progress(ui, t, *part, value);
                        }
                    }
                });
            }
        });
        ui.add_space(tokens::GAP);
    }
}

/// How hard a block is, how long one takes, and when that changes. Expert mode adds the whole
/// network's speed and the node's own counts.
fn pacing(ui: &mut Ui, cx: &Cx) {
    let t = cx.t;
    let voice = cx.voice();
    let node = &cx.snap.node;
    let rules = node.coin.as_ref().map(|coin| coin.rules.clone());
    let every = rules.as_ref().map_or(0, |rules| rules.retarget_interval);
    let aim_ms = rules.as_ref().map_or(0, |rules| rules.block_time_ms);
    let aim = format::seconds(voice, aim_ms.div_ceil(1_000).max(1));
    card(ui, t, |ui| {
        section(ui, cx.say("mining.pacing"));
        ui.add_space(tokens::GAP);
        let average = node.chain.average_ms.map(|ms| format::seconds(voice, ms.div_ceil(1_000).max(1)));
        let mut figures = vec![
            Figure::Plain(cx.say("mining.tries"), format::number(node.difficulty.into()), cx.say("mining.tries_under").to_string()),
            Figure::Plain(
                cx.say("mining.block_time"),
                cx.fill("mining.aim", &[&aim]),
                average.map(|average| cx.fill("mining.recent", &[&average])).unwrap_or_default(),
            ),
        ];
        figures.push(if every == 0 {
            Figure::Plain(cx.say("mining.next_change"), cx.say("mining.fixed").to_string(), String::new())
        } else {
            let since = node.height % every;
            let value = cx.fill("mining.in_blocks", &[&format::number((every - since).into())]);
            Figure::Track(cx.say("mining.next_change"), value, cx.fill("mining.every", &[&format::number(every.into())]), since as f32 / every as f32)
        });
        if cx.expert() {
            let all = format::rate(voice, node.chain.hashrate_all);
            figures.push(Figure::Plain(cx.say("pro.network_rate"), all, cx.fill("pro.peers_count", &[&node.peers.to_string()])));
        }
        figure_grid(ui, t, &figures);
        ui.add_space(tokens::GAP);
        let about = if every == 0 {
            cx.say("mining.pacing_fixed").to_string()
        } else {
            cx.fill("mining.pacing_about", &[&format::number(every.into()), &aim])
        };
        explain::short(ui, t, &about);
        explain::details(ui, t, "mining-pacing", details_labels(cx), |ui| {
            explain::analogy(ui, t, cx.say("mining.pacing_analogy"));
            if cx.expert() {
                ui.add_space(tokens::GAP);
                let facts = [
                    (cx.say("pro.mempool"), format::number(node.pool as u128)),
                    (cx.say("pro.utxo"), format::number(node.unspent as u128)),
                    (cx.say("pro.tip"), format::short_id(&node.tip)),
                    (cx.say("home.height"), format::number(node.height.into())),
                ];
                ui.horizontal_wrapped(|ui| {
                    for (name, value) in facts {
                        ui.label(RichText::new(name).color(t.muted));
                        ui.label(value);
                        ui.add_space(tokens::GAP_WIDE);
                    }
                });
            }
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
            pacing(ui, cx);
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
                pacing(ui, cx);
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
        explain::short(ui, t, cx.say("mining.together_about"));
        ui.add_space(tokens::GAP);
        self.pool.show(ui, cx);
    }
}
