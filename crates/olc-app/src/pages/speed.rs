//! How fast this computer mines: four steps of its full speed, a slider for anything between, and
//! in expert mode the exact number. The limit holds for every miner here, this app and the added
//! ones together, and the line under it says how they share it. Home and Mining draw the same
//! control, so the speed is set the same way wherever a person meets it. A change goes out once the
//! hand lets go.
use super::Cx;
use crate::core::Command;
use crate::ui::explain::{self, Choice};
use crate::ui::tokens;
use crate::ui::widgets::{button, meta, number_field, segments, slider, Kind};
use crate::ui::format;
use egui::{Align, FontId, Layout, RichText, Ui};

/// The steps, in percent of this computer's full speed. The last one lifts the limit.
pub const STEPS: [u64; 4] = [25, 50, 75, 100];
/// The slider's lowest point: below this a computer mines too slowly to see.
const LEAST: f32 = 5.0;
/// A step counts as chosen when the limit is this close to it, in percent.
const NEAR: f32 = 1.5;
/// A figure as wide as one computer's speed gets, so the title line keeps one shape from the first
/// frame, before the full speed is measured.
const WIDEST_RATE: u64 = 999_999;

/// The limit for a share of the full speed, where 0 means no limit.
pub fn cap_for(full: u64, percent: u64) -> u64 {
    if percent >= 100 {
        0
    } else {
        (full * percent / 100).max(1)
    }
}

/// The share of the full speed a limit allows, in percent.
fn percent_of(cap: u64, full: u64) -> f32 {
    if cap == 0 || full == 0 || cap >= full {
        100.0
    } else {
        cap as f32 * 100.0 / full as f32
    }
}

#[derive(Default)]
pub struct Speed {
    /// Where the knob is while a hand drags it.
    held: Option<f32>,
    /// The limit sent last, shown until the node says it took it.
    sent: Option<u64>,
    /// The number in the expert field, and the limit it was last set from.
    exact: u64,
    seen: u64,
}

fn apply(cx: &mut Cx, cap: u64) {
    cx.settings.hash_cap = cap;
    cx.send(Command::SetCap(cap));
    cx.save();
}

/// The limit against this computer's highest speed, as the speed control and Settings say it.
pub fn limit_words(cx: &Cx, cap: u64) -> String {
    let voice = cx.voice();
    let full = cx.snap.full_rate;
    match () {
        () if full == 0 => cx.say("rate.measuring").to_string(),
        () if cap == 0 || cap >= full => cx.fill("rate.max", &[&format::rate(voice, full)]),
        () => cx.fill("rate.of", &[&format::rate(voice, cap), &format::rate(voice, full)]),
    }
}

/// How this app and the added miners share the limit now, once there are added miners that mine.
pub fn split_line(ui: &mut Ui, cx: &Cx) {
    let budget = &cx.snap.budget;
    if budget.parts.is_empty() {
        return;
    }
    let voice = cx.voice();
    let added: u64 = budget.parts.iter().map(|(_, part)| part).sum();
    let line = cx.fill("budget.split", &[&format::rate(voice, budget.own), &format::rate(voice, added)]);
    meta(ui, &line, cx.t);
}

impl Speed {
    /// The person's limit for the whole computer: what was sent until the core holds it.
    fn cap_now(&mut self, cx: &Cx) -> u64 {
        let held = cx.snap.budget.limit;
        match self.sent {
            Some(sent) if sent != held => sent,
            _ => {
                self.sent = None;
                held
            }
        }
    }

    fn send(&mut self, cx: &mut Cx, cap: u64) {
        self.sent = Some(cap);
        apply(cx, cap);
    }

    /// The steps, the slider and the number beside them, with the line that says what it is for.
    pub fn show(&mut self, ui: &mut Ui, cx: &mut Cx, salt: &str) {
        let t = cx.t;
        let voice = cx.voice();
        let full = cx.snap.full_rate;
        let cap = self.cap_now(cx);
        let percent = self.held.unwrap_or_else(|| percent_of(cap, full));
        // The limit this control sets, against this computer's highest speed on its threads.
        let shown = match self.held {
            Some(held) if held < 100.0 => limit_words(cx, (full as f32 * held / 100.0).round() as u64),
            Some(_) => limit_words(cx, 0),
            None => limit_words(cx, cap),
        };
        // The name and the figure share a line when both fit whole; a narrow card puts the figure under.
        // The longest figure decides, so the slider does not jump a line while it is dragged.
        let wide = |text: &str, size: f32| {
            ui.painter().layout_no_wrap(text.to_string(), FontId::proportional(size), t.text).size().x
        };
        let widest = format::rate(voice, full.max(WIDEST_RATE));
        let longest = cx.fill("rate.of", &[&widest, &widest]);
        let figure_room = wide(&longest, tokens::META).max(wide(&shown, tokens::META));
        let room = wide(cx.say("budget.title"), tokens::TEXT) + figure_room + tokens::GAP_WIDE;
        let title = RichText::new(cx.say("budget.title")).color(t.text);
        let figure = RichText::new(shown).size(tokens::META).color(t.soft);
        if room <= ui.available_width() {
            ui.horizontal(|ui| {
                ui.label(title);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| ui.label(figure));
            });
        } else {
            ui.label(title);
            ui.label(figure);
        }
        let items: Vec<(u64, String)> = STEPS
            .iter()
            .map(|step| (*step, if *step == 100 { cx.say("rate.full").to_string() } else { format!("{step}%") }))
            .collect();
        let current = STEPS.iter().copied().find(|step| (*step as f32 - percent).abs() < NEAR);
        if let Some(step) = segments(ui, t, &items, current).filter(|_| full > 0) {
            self.held = None;
            self.send(cx, cap_for(full, step));
        }
        if full > 0 {
            let mut value = percent;
            let marks: Vec<f32> = STEPS[..3].iter().map(|step| (*step as f32 - LEAST) / (100.0 - LEAST)).collect();
            let slid = ui.push_id(("speed", salt), |ui| slider(ui, t, &mut value, LEAST..=100.0, cx.say("rate.slider"), &marks)).inner;
            if slid.response.dragged() {
                self.held = Some(value);
            }
            if slid.settled {
                self.held = None;
                let chosen = value.round() as u64;
                self.send(cx, cap_for(full, chosen));
            }
        }
        if cx.expert() {
            self.exact_row(ui, cx, cap);
        }
        split_line(ui, cx);
        explain::short(ui, t, cx.say("budget.about"));
        let labels = (cx.say("explain.details"), cx.say("explain.hide"));
        explain::details(ui, t, ("speed", salt), labels, |ui| steps_compared(ui, cx));
    }

    /// The limit as a number, taken when Apply is pressed, so typing does not change the speed
    /// one figure at a time.
    fn exact_row(&mut self, ui: &mut Ui, cx: &mut Cx, cap: u64) {
        let t = cx.t;
        ui.horizontal(|ui| {
            let name = cx.say("pro.rate_exact");
            ui.label(RichText::new(name).size(tokens::META).color(t.muted));
            if self.seen != cap {
                self.seen = cap;
                self.exact = cap;
            }
            number_field(ui, &mut self.exact, name, 120.0);
            if button(ui, t, cx.say("common.apply"), Kind::Plain).clicked() {
                let exact = self.exact;
                self.send(cx, exact);
            }
        });
        // Below the row, so in Home's narrow card it reads as one line instead of a word a line.
        meta(ui, cx.say("pro.rate_zero"), t);
    }
}

/// The four steps on the same three measures, worked out from this computer's own numbers.
fn steps_compared(ui: &mut Ui, cx: &Cx) {
    let voice = cx.voice();
    let full = cx.snap.full_rate;
    let difficulty = cx.snap.node.difficulty;
    let unknown = cx.say("rate.unknown");
    let names: Vec<String> =
        STEPS.iter().map(|step| if *step == 100 { cx.say("rate.full").to_string() } else { format!("{step}%") }).collect();
    let values: Vec<[String; 3]> = STEPS
        .iter()
        .map(|step| {
            let rate = full * step / 100;
            let share = format!("{step}%");
            let speed = if full == 0 { unknown.to_string() } else { format::rate(voice, rate) };
            let block = match (rate, difficulty) {
                (0, _) | (_, 0) => unknown.to_string(),
                (rate, difficulty) => format::seconds(voice, (difficulty / rate).max(1)),
            };
            [share, speed, block]
        })
        .collect();
    let rows: Vec<[&str; 3]> = values.iter().map(|row| [row[0].as_str(), row[1].as_str(), row[2].as_str()]).collect();
    let choices: Vec<Choice> = names.iter().zip(&rows).map(|(name, values)| Choice { name, values }).collect();
    let criteria = [cx.say("rate.compare_share"), cx.say("rate.compare_rate"), cx.say("rate.compare_block")];
    explain::compare(ui, cx.t, "speed-steps", &criteria, &choices);
    ui.add_space(tokens::GAP);
    meta(ui, cx.say("rate.compare_note"), cx.t);
}
