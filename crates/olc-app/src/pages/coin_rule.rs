//! The three difficulty rules a coin chooses from, as a picker named by what each does. Behind
//! Details: what a person would see on the block strip after mining changes, one row a rule on one
//! time axis, then the same criteria for each. The blocks came from real nodes
//! ([`super::coin_rule_data`]).
use super::coin_rule_data::{self, Run};
use super::Cx;
use crate::core::DIFFICULTY_RULES;
use crate::ui::explain::{self, Choice};
use crate::ui::register::{setting_details, When};
use crate::ui::tokens;
use crate::ui::widgets::{meta, segments};
use egui::{pos2, vec2, Align2, FontId, Id, Pos2, Rect, Sense, Stroke, Ui};

/// The picture's minutes after the change, in steps a person counts.
const AXIS_MINUTES: f64 = 120.0;
const AXIS_STEPS: [f64; 5] = [0.0, 30.0, 60.0, 90.0, 120.0];
/// Each rule's row: its name over a strip of blocks.
const NAME_ROW: f32 = 18.0;
const STRIP: f32 = 16.0;
const ROW_GAP: f32 = 10.0;
const AXIS_ROOM: f32 = 22.0;
const LEGEND_ROOM: f32 = 24.0;
/// Room after the axis for its last label.
const END_ROOM: f32 = 22.0;
/// A block is a bar this wide and tall; an easy block at the floor is half as tall.
const TICK: f32 = 2.0;
const TALL: f32 = 14.0;
const EASY: f32 = 7.0;
/// How strongly the stretch at pace is washed in the colour of things going well.
const PACE_WASH: f32 = 0.22;

/// A rule's name: in basic mode what it does, with the blocks between its adjustments; in expert
/// mode the name the trade uses.
pub fn rule_name(cx: &Cx, rule: &str, window: u64) -> String {
    let key = match rule {
        "bitcoin" => "coin.rule_bitcoin",
        "bip94" => "coin.rule_bip94",
        _ => "coin.rule_asert",
    };
    cx.fill(key, &[&window.to_string()])
}

fn used_in(cx: &Cx, rule: &str) -> String {
    let key = match rule {
        "bitcoin" => "coin.used_bitcoin",
        "bip94" => "coin.used_bip94",
        _ => "coin.used_asert",
    };
    cx.say(key).to_string()
}

/// Seconds as a person reads them: at once under a second, seconds under a minute, else minutes to
/// a tenth.
fn span(cx: &Cx, secs: f64) -> String {
    match secs {
        _ if secs < 1.0 => cx.say("coin.v_at_once").to_string(),
        _ if secs < 60.0 => cx.fill("coin.v_seconds", &[&format!("{secs:.0}")]),
        _ => cx.fill("coin.v_minutes", &[&format!("{:.1}", secs / 60.0)]),
    }
}

/// A wait to expect, in whole minutes when it is whole.
fn about(cx: &Cx, secs: f64) -> String {
    let minutes = secs / 60.0;
    match minutes >= 1.0 && minutes.fract() == 0.0 {
        true => cx.fill("coin.v_minutes", &[&format!("{minutes:.0}")]),
        false => span(cx, secs),
    }
}

/// Until the first block after the change. With no easy block for a late one, the block mined at
/// the old difficulty takes as many times the target as mining fell, so that is what to expect and
/// one run's luck is the smaller line under it; with easy blocks the floor sets it, as measured.
fn first(cx: &Cx, run: &Run, case: &str, easy: bool) -> String {
    let Some(measured) = run.first() else {
        return String::new();
    };
    match coin_rule_data::drop_factor(case).filter(|_| !easy) {
        Some(times) => {
            let usual = cx.fill("coin.v_usual", &[&about(cx, times * run.target)]);
            format!("{usual}\n{}", cx.fill("coin.v_this_time", &[&span(cx, measured)]))
        }
        None => span(cx, measured),
    }
}

/// How long a run was watched, in whole hours when it was.
fn watched(cx: &Cx, secs: f64) -> String {
    let hours = secs / 3_600.0;
    match hours >= 1.0 && hours.fract() == 0.0 {
        true => cx.fill("coin.v_hours", &[&format!("{hours:.0}")]),
        false => span(cx, secs),
    }
}

/// Whether a rule lets a late block come easy: Bitcoin's never, BIP 94's always, the per-block
/// rule as the coin says.
fn lets_late_block_come_easy(rule: &str, late_floor: bool) -> bool {
    match rule {
        "bitcoin" => false,
        "bip94" => true,
        _ => late_floor,
    }
}

fn case_name(cx: &Cx, case: &str) -> String {
    coin_rule_data::case_words(case).map_or_else(|| case.to_string(), |(key, number)| cx.fill(key, &[number]))
}

/// The picker as a line of the form. `now` is the rule the coin would have, `window` its blocks
/// between retargets and `late_floor` whether the per-block rule lets a late block come easy.
/// Returns the rule chosen.
pub fn rule_row(ui: &mut Ui, cx: &Cx, now: &str, window: u64, late_floor: bool) -> Option<&'static str> {
    let t = cx.t;
    let items: Vec<(&'static str, String)> = DIFFICULTY_RULES.iter().map(|rule| (*rule, rule_name(cx, rule, window))).collect();
    let current = DIFFICULTY_RULES.iter().copied().find(|rule| *rule == now);
    let mut chosen = None;
    let name = (cx.say("coin.rule"), (When::Now, ""));
    let labels = (cx.say("explain.details"), cx.say("explain.hide"));
    let control = |ui: &mut Ui| chosen = segments(ui, t, &items, current);
    setting_details(ui, t, name, (cx.say("coin.rule_about"), "difficulty-rules", labels), control, |ui| {
        let Some(case) = case_choice(ui, cx) else {
            return;
        };
        picture(ui, cx, current, case, window);
        ui.add_space(tokens::GAP_WIDE);
        table(ui, cx, case, (window, late_floor));
        ui.add_space(tokens::GAP);
        let target = DIFFICULTY_RULES.iter().find_map(|rule| coin_rule_data::run(rule, case)).map_or(0.0, |run| run.target);
        meta(ui, &cx.fill("coin.rule_measured", &[&format!("{target:.0}")]), t);
    });
    chosen
}

/// Which change of mining the picture shows. A tray to switch when more than one was measured.
fn case_choice(ui: &mut Ui, cx: &Cx) -> Option<&'static str> {
    let cases = coin_rule_data::cases();
    let id = Id::unique("difficulty-case");
    let mut shown = ui.ctx().data(|data| data.get_temp::<usize>(id)).unwrap_or(0).min(cases.len().saturating_sub(1));
    if cases.len() > 1 {
        let items: Vec<(usize, String)> = cases.iter().enumerate().map(|(index, case)| (index, case_name(cx, case))).collect();
        if let Some(index) = segments(ui, cx.t, &items, Some(shown)) {
            shown = index;
            ui.ctx().data_mut(|data| data.insert_temp(id, index));
        }
        ui.add_space(tokens::GAP);
    }
    cases.get(shown).copied()
}

/// One row a rule on one time axis, a line at the moment mining changed: a tall bar for each block
/// when it came, a short one for an easy block at the floor, and a wash from where blocks came at
/// their pace again. A rule not back while watched says so on its row. Blocks from before the
/// change sit left of the line. The rule picked above is named in full white.
fn picture(ui: &mut Ui, cx: &Cx, current: Option<&str>, case: &str, window: u64) {
    let t = cx.t;
    let row = NAME_ROW + STRIP + ROW_GAP;
    let rows: f32 = DIFFICULTY_RULES.iter().map(|_| row).sum();
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), rows + AXIS_ROOM + LEGEND_ROOM), Sense::hover());
    let painter = ui.painter();
    let font = FontId::proportional(tokens::META);
    let (left, right) = (rect.left(), rect.right() - END_ROOM);
    let horizon = AXIS_MINUTES * 60.0;
    let from = (coin_rule_data::earliest(case) / 60.0).floor() * 60.0;
    let x = |secs: f64| left + (right - left) * ((secs - from) / (horizon - from)).clamp(0.0, 1.0) as f32;
    let mut top = rect.top();
    let mut any_easy = false;
    for rule in DIFFICULTY_RULES {
        let colour = if current == Some(rule) { t.text } else { t.soft };
        painter.text(pos2(left, top), Align2::LEFT_TOP, rule_name(cx, rule, window), font.clone(), colour);
        let (strip_top, base) = (top + NAME_ROW, top + NAME_ROW + STRIP);
        if let Some(run) = coin_rule_data::run(rule, case) {
            if run.back.is_none() {
                painter.text(pos2(rect.right(), top), Align2::RIGHT_TOP, never(cx, run), font.clone(), t.warn);
            }
            if let Some(back) = run.back.filter(|back| *back <= horizon) {
                let wash = Rect::from_min_max(pos2(x(back), strip_top), pos2(right, base));
                painter.rect_filled(wash, 2.0, t.good.gamma_multiply(PACE_WASH));
            }
            painter.line_segment([pos2(left, base), pos2(right, base)], Stroke::new(1.0, t.line_strong));
            for block in run.blocks.iter().filter(|block| (from..=horizon).contains(&block.secs)) {
                let (height, colour) = if block.easy { (EASY, t.muted) } else { (TALL, t.text) };
                any_easy |= block.easy;
                let middle = x(block.secs);
                painter.rect_filled(Rect::from_min_max(pos2(middle - TICK / 2.0, base - height), pos2(middle + TICK / 2.0, base)), 0.0, colour);
            }
        }
        painter.line_segment([pos2(x(0.0), strip_top - 2.0), pos2(x(0.0), base)], Stroke::new(1.0, t.muted));
        top += row;
    }
    let axis = top - ROW_GAP / 2.0 + AXIS_ROOM / 2.0;
    for step in AXIS_STEPS {
        let place = if step == 0.0 && from == 0.0 { Align2::LEFT_CENTER } else { Align2::CENTER_CENTER };
        let said = cx.fill("coin.v_minutes", &[&format!("{step:.0}")]);
        painter.text(pos2(x(step * 60.0), axis), place, said, font.clone(), t.muted);
    }
    legend(ui, cx, pos2(left, rect.bottom() - LEGEND_ROOM / 2.0), any_easy);
    let name = cx.say("coin.rule_picture");
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, name));
}

/// What the marks mean, under the axis: a block, an easy block when there were any, the wash.
fn legend(ui: &Ui, cx: &Cx, at: Pos2, any_easy: bool) {
    let t = cx.t;
    let painter = ui.painter();
    let font = FontId::proportional(tokens::META);
    let mut from = at.x;
    let base = at.y + TALL / 2.0;
    let marks = [(TALL, t.text, "coin.legend_block", true), (EASY, t.muted, "coin.legend_easy", any_easy)];
    for (height, colour, key, _) in marks.into_iter().filter(|mark| mark.3) {
        let bar = Rect::from_min_max(pos2(from, base - height), pos2(from + TICK, base));
        painter.rect_filled(bar, 0.0, colour);
        let said = painter.text(pos2(from + TICK + tokens::GAP, at.y), Align2::LEFT_CENTER, cx.say(key), font.clone(), t.soft);
        from = said.right() + tokens::GAP_WIDE;
    }
    let wash = Rect::from_min_max(pos2(from, base - TALL), pos2(from + 16.0, base));
    painter.rect_filled(wash, 2.0, t.good.gamma_multiply(PACE_WASH));
    painter.text(pos2(wash.right() + tokens::GAP, at.y), Align2::LEFT_CENTER, cx.say("coin.legend_pace"), font, t.soft);
}

/// The three rules on the same criteria: where each is used, in expert mode when it adjusts, the
/// first block after the change and how long until blocks came at their pace.
fn table(ui: &mut Ui, cx: &Cx, case: &str, (window, late_floor): (u64, bool)) {
    let every = window.to_string();
    let values: Vec<Vec<String>> = DIFFICULTY_RULES
        .iter()
        .map(|rule| {
            let run = coin_rule_data::run(rule, case);
            let mut row = vec![used_in(cx, rule)];
            if cx.expert() {
                row.push(match *rule {
                    "bitcoin" => cx.fill("coin.v_every", &[&every]),
                    "bip94" => cx.fill("coin.v_every_wait", &[&every]),
                    _ => cx.say("coin.v_each").to_string(),
                });
            }
            let easy = lets_late_block_come_easy(rule, late_floor);
            row.push(cx.say(if easy { "coin.v_yes" } else { "coin.v_no" }).to_string());
            row.push(run.map(|run| first(cx, run, case, easy)).unwrap_or_default());
            row.push(run.map(|run| back(cx, run)).unwrap_or_default());
            row
        })
        .collect();
    let shown: Vec<Vec<&str>> = values.iter().map(|row| row.iter().map(String::as_str).collect()).collect();
    let names: Vec<String> = DIFFICULTY_RULES.iter().map(|rule| rule_name(cx, rule, window)).collect();
    let choices: Vec<Choice> = names.iter().zip(&shown).map(|(name, values)| Choice { name, values }).collect();
    let mut criteria = vec![cx.say("coin.c_used")];
    if cx.expert() {
        criteria.push(cx.say("coin.c_when"));
    }
    criteria.extend([cx.say("coin.c_easy"), cx.say("coin.c_first"), cx.say("coin.c_back")]);
    explain::compare(ui, cx.t, "difficulty-rules", &criteria, &choices);
}

fn back(cx: &Cx, run: &Run) -> String {
    match (run.back, run.blocks_to_back()) {
        (Some(secs), Some(blocks)) => cx.fill("coin.v_back", &[&span(cx, secs), &blocks.to_string()]),
        _ => never(cx, run),
    }
}

/// Not back while the run was watched, the picture's two hours when the file does not say.
fn never(cx: &Cx, run: &Run) -> String {
    cx.fill("coin.v_never", &[&watched(cx, run.watched.unwrap_or(AXIS_MINUTES * 60.0))])
}

#[cfg(test)]
mod tests {
    use super::*;
    use olc_core::coin::CoinConfig;

    /// The table's late-block row says what each measured run did, OLC's own floor for the
    /// per-block rule included.
    #[test]
    fn a_drop_with_no_easy_block_expects_the_target_times_the_fall() {
        assert_eq!(coin_rule_data::drop_factor("drop20"), Some(20.0));
        assert_eq!(coin_rule_data::drop_factor("rise50"), None);
    }

    #[test]
    fn the_late_block_row_matches_the_runs() {
        let olc = CoinConfig::olc().rules.allow_min_difficulty_blocks;
        for case in coin_rule_data::cases() {
            for rule in DIFFICULTY_RULES {
                let run = coin_rule_data::run(rule, case).unwrap_or_else(|| panic!("{rule} {case}"));
                assert_eq!(run.late_floor, Some(lets_late_block_come_easy(rule, olc)), "{rule} {case}");
                if !lets_late_block_come_easy(rule, olc) {
                    assert!(run.blocks.iter().all(|block| !block.easy), "{rule} {case} has no easy block");
                }
            }
        }
    }
}
