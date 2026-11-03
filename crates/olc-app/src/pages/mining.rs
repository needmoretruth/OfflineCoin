//! Mining on this computer, then everything that adds to it. First this computer's own miner with
//! its speed and the race it runs against everyone else, then how blocks are paced, then more
//! miners on this computer, then boards as add-ons, then mining together.
use super::boards::state_colour;
use super::local::Local;
use super::mining_card::MiningCard;
use super::pool::PoolTab;
use super::{Cx, Page};
use crate::core::unix_ms;
use crate::ui::chain::{self, Look};
use crate::ui::explain;
use crate::ui::theme::desktop_reduces_motion;
use crate::ui::tokens::{self, Tokens};
use crate::ui::widgets::{self, button, card, dot, meta, section, Kind};
use crate::ui::{format, Icon};
use egui::{pos2, vec2, Align, CornerRadius, Label, Layout, Rect, RichText, Sense, Ui};

/// How wide this computer's card takes of the first row; the race takes the rest.
const COMPUTER_SHARE: f32 = 0.5;
const STAT_TRACK: f32 = 6.0;

#[derive(Default)]
pub struct Mining {
    card: MiningCard,
    local: Local,
    pool: PoolTab,
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
            ui.add(Label::new(RichText::new(under).size(tokens::META).color(t.soft)).truncate());
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
        let count = if cx.expert() { 4.0 } else { 3.0 };
        let width = (ui.available_width() - tokens::GAP_WIDE * (count - 1.0)) / count;
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = tokens::GAP_WIDE;
            ui.allocate_ui_with_layout(vec2(width, 0.0), Layout::top_down(Align::Min), |ui| {
                ui.set_width(width);
                stat(ui, t, cx.say("mining.tries"), &format::number(node.difficulty.into()), cx.say("mining.tries_under"));
            });
            ui.allocate_ui_with_layout(vec2(width, 0.0), Layout::top_down(Align::Min), |ui| {
                ui.set_width(width);
                let average = node.chain.average_ms.map(|ms| format::seconds(voice, ms.div_ceil(1_000).max(1)));
                let under = average.map(|average| cx.fill("mining.recent", &[&average])).unwrap_or_default();
                stat(ui, t, cx.say("mining.block_time"), &cx.fill("mining.aim", &[&aim]), &under);
            });
            ui.allocate_ui_with_layout(vec2(width, 0.0), Layout::top_down(Align::Min), |ui| {
                ui.set_width(width);
                if every == 0 {
                    stat(ui, t, cx.say("mining.next_change"), cx.say("mining.fixed"), "");
                } else {
                    let since = node.height % every;
                    let left = every - since;
                    let value = cx.fill("mining.in_blocks", &[&format::number(left.into())]);
                    stat(ui, t, cx.say("mining.next_change"), &value, &cx.fill("mining.every", &[&format::number(every.into())]));
                    ui.add_space(4.0);
                    progress(ui, t, since as f32 / every as f32, &value);
                }
            });
            if cx.expert() {
                ui.allocate_ui_with_layout(vec2(width, 0.0), Layout::top_down(Align::Min), |ui| {
                    ui.set_width(width);
                    let all = format::rate(voice, node.chain.hashrate_all);
                    stat(ui, t, cx.say("pro.network_rate"), &all, &cx.fill("pro.peers_count", &[&node.peers.to_string()]));
                });
            }
        });
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
    fn first_row(&mut self, ui: &mut Ui, cx: &mut Cx, still: bool) {
        let heading = cx.say("mining.computer");
        if ui.available_width() < tokens::COMPACT {
            self.card.show(ui, cx, heading, "mining", still);
            ui.add_space(tokens::GAP_WIDE);
            race(ui, cx, still);
            return;
        }
        let total = ui.available_width();
        let left = ((total - tokens::GAP_WIDE) * COMPUTER_SHARE).round();
        let right = total - left - tokens::GAP_WIDE;
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(vec2(left, 0.0), Layout::top_down(Align::Min), |ui| {
                ui.set_width(left);
                self.card.show(ui, cx, heading, "mining", still);
            });
            ui.add_space(tokens::GAP_WIDE - ui.spacing().item_spacing.x);
            ui.allocate_ui_with_layout(vec2(right, 0.0), Layout::top_down(Align::Min), |ui| {
                ui.set_width(right);
                race(ui, cx, still);
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
        ui.add_space(tokens::GAP_WIDE);
        pacing(ui, cx);
        ui.add_space(tokens::GAP_PAGE);
        section(ui, cx.say("mining.more"));
        explain::short(ui, t, cx.say("mining.more_about"));
        ui.add_space(tokens::GAP);
        self.local.show(ui, cx);
        ui.add_space(tokens::GAP_PAGE);
        Self::boards(ui, cx);
        ui.add_space(tokens::GAP_PAGE);
        section(ui, cx.say("mining.together"));
        explain::short(ui, t, cx.say("mining.together_about"));
        ui.add_space(tokens::GAP);
        self.pool.show(ui, cx);
    }
}
