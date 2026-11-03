//! The three difficulty rules a coin chooses from, as a picker. Behind Details: a picture of how
//! long each took to get back to its pace after the miners left, then the same three criteria for
//! each. The numbers are the ones measured on 2026-11-04 with three real nodes at OLC's numbers:
//! 30 second blocks, windows of 10, mining dropped from 20,000 to 1,000 tries a second
//! (docs/technical.md, Difficulty rules).
use super::Cx;
use crate::core::DIFFICULTY_RULES;
use crate::ui::explain::{self, Choice};
use crate::ui::register::{setting_details, When};
use crate::ui::tokens;
use crate::ui::widgets::{meta, segments};
use egui::{pos2, vec2, Align2, FontId, Rect, Sense, Ui};

/// What was measured for one rule, in minutes after the drop: its first block, and the first
/// block its own rule asked at the usual pace, with the blocks it took to get there.
struct Measured {
    first: f32,
    back: f32,
    blocks: u32,
}

/// In the order of [`DIFFICULTY_RULES`]: Bitcoin's, BIP 94's and ASERT.
const MEASURED: [Measured; 3] = [
    Measured { first: 30.8, back: 110.5, blocks: 25 },
    Measured { first: 1.0, back: 56.3, blocks: 45 },
    Measured { first: 11.5, back: 16.3, blocks: 8 },
];

/// The picture's minutes, a little past the slowest, in steps a person counts.
const AXIS_MINUTES: f32 = 120.0;
const AXIS_STEP: f32 = 30.0;
/// The rule's name on the left, and room for the minutes after the longest bar.
const NAME_WIDTH: f32 = 150.0;
const VALUE_ROOM: f32 = 96.0;
const BAR_ROW: f32 = 30.0;
const BAR: f32 = 10.0;
const AXIS_ROOM: f32 = 24.0;
const LEGEND_ROOM: f32 = 24.0;
const SWATCH: f32 = 12.0;

/// A rule's name, by its name in the coin file.
pub fn name_key(rule: &str) -> &'static str {
    match rule {
        "bitcoin" => "coin.rule_bitcoin",
        "bip94" => "coin.rule_bip94",
        _ => "coin.rule_asert",
    }
}

fn minutes(cx: &Cx, value: f32) -> String {
    cx.fill("coin.v_minutes", &[&format!("{value:.1}")])
}

/// The picker as a line of the form. `now` is the rule the coin would have and `window` its blocks
/// between retargets. Returns the rule chosen.
pub fn rule_row(ui: &mut Ui, cx: &Cx, now: &str, window: u64) -> Option<&'static str> {
    let t = cx.t;
    let items: Vec<(&'static str, String)> = DIFFICULTY_RULES.iter().map(|rule| (*rule, cx.say(name_key(rule)).to_string())).collect();
    let current = DIFFICULTY_RULES.iter().copied().find(|rule| *rule == now);
    let mut chosen = None;
    let name = (cx.say("coin.rule"), (When::Now, ""));
    let labels = (cx.say("explain.details"), cx.say("explain.hide"));
    let control = |ui: &mut Ui| chosen = segments(ui, t, &items, current);
    setting_details(ui, t, name, (cx.say("coin.rule_about"), "difficulty-rules", labels), control, |ui| {
        picture(ui, cx, current);
        ui.add_space(tokens::GAP_WIDE);
        table(ui, cx, window);
        ui.add_space(tokens::GAP);
        meta(ui, cx.say("coin.rule_measured"), t);
    });
    chosen
}

/// One bar a rule: the grey part is the wait for the first block, the bright part the slow blocks
/// after it, and the bar ends where blocks come at their pace again. The rule picked above is named
/// in full white, the others a step softer, so the eye finds it.
fn picture(ui: &mut Ui, cx: &Cx, current: Option<&str>) {
    let t = cx.t;
    let width = ui.available_width();
    let rows = MEASURED.len() as f32;
    let (rect, response) = ui.allocate_exact_size(vec2(width, BAR_ROW * rows + AXIS_ROOM + LEGEND_ROOM), Sense::hover());
    let painter = ui.painter();
    let font = FontId::proportional(tokens::META);
    let (left, right) = (rect.left() + NAME_WIDTH, rect.right() - VALUE_ROOM);
    let x = |value: f32| left + (right - left) * (value / AXIS_MINUTES).min(1.0);
    for (index, (rule, measured)) in DIFFICULTY_RULES.iter().zip(&MEASURED).enumerate() {
        let middle = rect.top() + BAR_ROW * (index as f32 + 0.5);
        let bar = |from: f32, to: f32| Rect::from_min_max(pos2(x(from), middle - BAR / 2.0), pos2(x(to), middle + BAR / 2.0));
        let named = if current == Some(*rule) { t.text } else { t.soft };
        painter.text(pos2(rect.left(), middle), Align2::LEFT_CENTER, cx.say(name_key(rule)), font.clone(), named);
        painter.rect_filled(bar(0.0, measured.back), 2.0, t.text);
        painter.rect_filled(bar(0.0, measured.first), 2.0, t.muted);
        let end = pos2(x(measured.back) + tokens::GAP, middle);
        painter.text(end, Align2::LEFT_CENTER, minutes(cx, measured.back), font.clone(), t.muted);
    }
    let axis = rect.top() + BAR_ROW * rows + AXIS_ROOM / 2.0;
    let mut step = 0.0;
    while step <= AXIS_MINUTES {
        let label = cx.fill("coin.v_minutes", &[&format!("{step:.0}")]);
        painter.text(pos2(x(step), axis), Align2::CENTER_CENTER, label, font.clone(), t.muted);
        step += AXIS_STEP;
    }
    let legend = rect.bottom() - LEGEND_ROOM / 2.0;
    let mut from = left;
    for (colour, key) in [(t.muted, "coin.rule_waiting"), (t.text, "coin.rule_slow")] {
        let swatch = Rect::from_center_size(pos2(from + SWATCH / 2.0, legend), vec2(SWATCH, BAR));
        painter.rect_filled(swatch, 2.0, colour);
        let said = painter.text(pos2(swatch.right() + tokens::GAP, legend), Align2::LEFT_CENTER, cx.say(key), font.clone(), t.soft);
        from = said.right() + tokens::GAP_WIDE;
    }
    let name = cx.say("coin.rule_picture");
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, name));
}

/// The three rules on when they adjust, the first block after the miners left, and how long until
/// blocks came at their pace.
fn table(ui: &mut Ui, cx: &Cx, window: u64) {
    let every = window.to_string();
    let values: Vec<[String; 3]> = DIFFICULTY_RULES
        .iter()
        .zip(&MEASURED)
        .map(|(rule, measured)| {
            let when = match *rule {
                "bitcoin" => cx.fill("coin.v_every", &[&every]),
                "bip94" => cx.fill("coin.v_every_wait", &[&every]),
                _ => cx.say("coin.v_each").to_string(),
            };
            let back = cx.fill("coin.v_back", &[&format!("{:.1}", measured.back), &measured.blocks.to_string()]);
            [when, minutes(cx, measured.first), back]
        })
        .collect();
    let shown: Vec<Vec<&str>> = values.iter().map(|row| row.iter().map(String::as_str).collect()).collect();
    let names: Vec<&str> = DIFFICULTY_RULES.iter().map(|rule| cx.say(name_key(rule))).collect();
    let choices: Vec<Choice> = names.iter().zip(&shown).map(|(name, values)| Choice { name, values }).collect();
    let criteria = [cx.say("coin.c_when"), cx.say("coin.c_first"), cx.say("coin.c_back")];
    explain::compare(ui, cx.t, "difficulty-rules", &criteria, &choices);
}
