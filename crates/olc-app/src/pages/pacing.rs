//! How blocks are paced on the coin this computer mines: how hard a block is, how long one takes,
//! and how that follows the speed. The coin's own rules on the chain say how, so the card shows
//! the wait for the next change only for a coin that changes every so many blocks, and how the
//! last blocks moved it for a coin that changes at every block.
use super::Cx;
use crate::ui::chain;
use crate::ui::explain;
use crate::ui::format;
use crate::ui::paint;
use crate::ui::tokens::{self, Tokens};
use crate::ui::widgets::{card, section};
use egui::{pos2, vec2, Align, CornerRadius, Label, Layout, Rect, RichText, Sense, Ui};
use olc_core::{DifficultyRule, Rules};

const STAT_TRACK: f32 = 6.0;
/// A figure in the pacing card needs this much room; fewer fit in a row on a narrow column.
const STAT_WIDTH: f32 = 150.0;
/// The blocks whose moves the card shows for a coin that changes at every block.
const MOVE_BLOCKS: usize = 20;
const MOVES_HEIGHT: f32 = 26.0;
/// The shortest column, so the easiest block still shows.
const MOVE_FLOOR: f32 = 4.0;
const MOVE_GAP: f32 = 2.0;

/// How a coin's difficulty follows the speed, as its rules on the chain say.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rule {
    /// The same for every block.
    Fixed,
    /// Once every so many blocks, from how long they took, as Bitcoin does every 2016.
    Window(u64),
    /// At every block, from how long the last ones took.
    EveryBlock,
}

impl Rule {
    /// The rule a coin's rules name. A window of one block changes it at every block.
    /// An interval of 0 holds the difficulty under every rule, as the core does; per-block
    /// adjustment moves it at every block whatever the interval says.
    pub fn of(rules: &Rules) -> Self {
        match (rules.retarget_interval, rules.difficulty_rule) {
            (0, _) => Rule::Fixed,
            (_, DifficultyRule::Asert) | (1, _) => Rule::EveryBlock,
            (every, _) => Rule::Window(every),
        }
    }
}

/// How far the difficulty moved from the oldest to the newest of these blocks, in whole percent.
pub fn moved_percent(oldest: u64, newest: u64) -> i64 {
    if oldest == 0 {
        return 0;
    }
    let change = (i128::from(newest) - i128::from(oldest)) * 100;
    let half = i128::from(oldest) / 2;
    let rounded = if change >= 0 { (change + half) / i128::from(oldest) } else { (change - half) / i128::from(oldest) };
    i64::try_from(rounded).unwrap_or(if rounded > 0 { i64::MAX } else { i64::MIN })
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

/// The difficulty of the last blocks, oldest on the left, one column each: how each block moved
/// it. The newest stands out, as it is the one the next block follows.
fn moves(ui: &mut Ui, t: &Tokens, difficulties: &[u64], name: &str) {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(vec2(width, MOVES_HEIGHT), Sense::hover());
    let count = difficulties.len().max(1) as f32;
    let column = ((width - MOVE_GAP * (count - 1.0)) / count).max(1.0);
    let low = difficulties.iter().copied().min().unwrap_or(0);
    let high = difficulties.iter().copied().max().unwrap_or(0);
    let gray = chain::faces(t, false);
    let (start, end) = gray.bar(t);
    for (index, difficulty) in difficulties.iter().enumerate() {
        let part = if high > low { (difficulty - low) as f32 / (high - low) as f32 } else { 0.5 };
        let height = MOVE_FLOOR + part * (MOVES_HEIGHT - MOVE_FLOOR);
        let left = rect.left() + index as f32 * (column + MOVE_GAP);
        let bar = Rect::from_min_max(pos2(left, rect.bottom() - height), pos2(left + column, rect.bottom()));
        if index + 1 == difficulties.len() {
            ui.painter().rect_filled(bar, 2.0, t.soft);
        } else {
            ui.painter().add(paint::gradient(bar, 2.0, start, end));
        }
    }
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, name));
}

/// One figure of the pacing card, some with a picture under it.
enum Figure<'a> {
    Plain(&'a str, String, String),
    Track(&'a str, String, String, f32),
    Moves(&'a str, String, String, Vec<u64>),
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
                        Figure::Moves(name, value, under, difficulties) => {
                            stat(ui, t, name, value, under);
                            ui.add_space(4.0);
                            moves(ui, t, difficulties, value);
                        }
                    }
                });
            }
        });
        ui.add_space(tokens::GAP);
    }
}

/// How the coin's rule changes the difficulty: a wait, a move, or never.
fn change_figure<'a>(cx: &'a Cx, rule: Rule) -> Figure<'a> {
    let node = &cx.snap.node;
    match rule {
        Rule::Fixed => Figure::Plain(cx.say("mining.next_change"), cx.say("mining.fixed").to_string(), String::new()),
        Rule::Window(every) => {
            let since = node.height % every;
            let value = match every - since {
                1 => cx.say("mining.at_next").to_string(),
                left => cx.fill("mining.in_blocks", &[&format::number(left.into())]),
            };
            let under = cx.fill("mining.every", &[&format::number(every.into())]);
            Figure::Track(cx.say("mining.next_change"), value, under, since as f32 / every as f32)
        }
        Rule::EveryBlock => {
            let mut difficulties: Vec<u64> = node.chain.latest.iter().take(MOVE_BLOCKS).map(|block| block.difficulty).collect();
            difficulties.reverse();
            let (oldest, newest) = (difficulties.first().copied().unwrap_or(0), difficulties.last().copied().unwrap_or(0));
            let percent = moved_percent(oldest, newest);
            let value = match percent {
                0 => cx.say("mining.same").to_string(),
                up if up > 0 => cx.fill("mining.harder", &[&format::number(up.unsigned_abs().into())]),
                down => cx.fill("mining.easier", &[&format::number(down.unsigned_abs().into())]),
            };
            // One block alone has moved nothing yet, so it says no span.
            let under = match difficulties.len() {
                0 | 1 => String::new(),
                count => cx.fill("mining.over_blocks", &[&format::number(count as u128)]),
            };
            Figure::Moves(cx.say("mining.moved"), value, under, difficulties)
        }
    }
}

/// How hard a block is, how long one takes, and how that follows the speed. Expert mode adds the
/// whole network's speed and the node's own counts.
pub fn pacing_card(ui: &mut Ui, cx: &Cx, labels: (&str, &str)) {
    let t = cx.t;
    let voice = cx.voice();
    let node = &cx.snap.node;
    let rule = node.coin.as_ref().map_or(Rule::Fixed, |coin| Rule::of(&coin.rules));
    let aim_ms = node.coin.as_ref().map_or(0, |coin| coin.rules.block_time_ms);
    let aim = format::seconds(voice, aim_ms.div_ceil(1_000).max(1));
    card(ui, t, |ui| {
        section(ui, cx.say("mining.pacing"));
        ui.add_space(tokens::GAP);
        let average = node.chain.average_ms.map(|ms| format::seconds(voice, ms.div_ceil(1_000).max(1)));
        let mut figures = vec![
            // "Tries per block: 18,486 on average" for anyone; an expert reads the difficulty and
            // what it stands for.
            Figure::Plain(
                cx.say("mining.tries"),
                cx.fill("mining.tries_value", &[&format::number(node.difficulty.into())]),
                if cx.expert() { cx.say("pro.tries_under").to_string() } else { String::new() },
            ),
            Figure::Plain(
                cx.say("mining.block_time"),
                cx.fill("mining.aim", &[&aim]),
                average.map(|average| cx.fill("mining.recent", &[&average])).unwrap_or_default(),
            ),
            change_figure(cx, rule),
        ];
        if cx.expert() {
            let all = super::network::network_rate(cx);
            figures.push(Figure::Plain(cx.say("pro.network_rate"), all, cx.fill("pro.peers_count", &[&node.peers.to_string()])));
        }
        figure_grid(ui, t, &figures);
        ui.add_space(tokens::GAP);
        let about = match rule {
            Rule::Fixed => cx.say("mining.pacing_fixed").to_string(),
            Rule::Window(every) => cx.fill("mining.pacing_about", &[&format::number(every.into()), &aim]),
            Rule::EveryBlock => cx.fill("mining.pacing_each", &[&aim]),
        };
        explain::short(ui, t, &about);
        explain::details(ui, t, "mining-pacing", labels, |ui| {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rule_comes_from_the_coin() {
        let mut rules = olc_core::CoinConfig::olc().rules;
        rules.retarget_interval = 0;
        assert_eq!(Rule::of(&rules), Rule::Fixed);
        rules.retarget_interval = 1;
        assert_eq!(Rule::of(&rules), Rule::EveryBlock);
        rules.difficulty_rule = DifficultyRule::Bitcoin;
        rules.retarget_interval = 20;
        assert_eq!(Rule::of(&rules), Rule::Window(20));
        rules.difficulty_rule = DifficultyRule::Bip94;
        assert_eq!(Rule::of(&rules), Rule::Window(20));
        rules.difficulty_rule = DifficultyRule::Asert;
        assert_eq!(Rule::of(&rules), Rule::EveryBlock, "the interval does not hold a per-block rule");
        rules.retarget_interval = 0;
        assert_eq!(Rule::of(&rules), Rule::Fixed);
    }

    #[test]
    fn moves_round_to_whole_percent() {
        assert_eq!(moved_percent(100, 112), 12);
        assert_eq!(moved_percent(100, 88), -12);
        assert_eq!(moved_percent(1_000, 1_004), 0);
        assert_eq!(moved_percent(0, 50), 0);
        assert_eq!(moved_percent(200, 101), -50);
    }
}
