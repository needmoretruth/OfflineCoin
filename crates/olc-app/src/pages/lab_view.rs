//! What the Lab shows of a run on this computer: the choice, at most three lines of words, the two
//! chains as they race, and behind "Details" each step with its picture, a legend, an analogy and
//! the two experiments side by side. Drawing only: it takes words and a state and gives back what
//! was pressed, so a screen test can draw any state of a real run.
use super::lab_strip::{self, LIVE, SMALL};
use super::lab_words::{now_words, step_words};
use crate::i18n::{fill, text, Voice};
use crate::local::lab::{LabChoice, LabKind, LabState, Mark, Phase, Side};
use crate::settings::Mode;
use crate::ui::tokens::{self, Tokens};
use crate::ui::explain::{self, Choice};
use crate::ui::widgets::{button, button_on, card, number_field, section, segments, slider, switch_row, Kind};
use egui::{RichText, Ui};
use olc_lab::Outcome;

/// The share slider's width beside its name.
const SHARE_WIDTH: f32 = 220.0;

/// What a press on the Lab asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    Start,
    Stop,
    RemoveKept,
}

/// Everything the Lab draws from.
#[derive(Clone, Copy)]
pub struct Seen<'a> {
    pub state: Option<&'a LabState>,
    pub running: bool,
    pub kept: bool,
}

/// How many miners a run of this choice starts.
fn miners(choice: &LabChoice) -> usize {
    match choice.kind {
        LabKind::Attack if choice.helper => 3,
        LabKind::Attack => 2,
        LabKind::Split => 3,
    }
}

fn choose(ui: &mut Ui, t: &Tokens, voice: Voice, choice: &mut LabChoice, running: bool) {
    let kinds = [
        (LabKind::Attack, text(voice, "lab2.kind_attack").to_string()),
        (LabKind::Split, text(voice, "lab2.kind_split").to_string()),
    ];
    ui.add_enabled_ui(!running, |ui| {
        if let Some(kind) = segments(ui, t, &kinds, Some(choice.kind)) {
            choice.kind = kind;
        }
        ui.add_space(tokens::GAP);
        let (label, line) = match choice.kind {
            LabKind::Attack => ("lab2.share_attack", "lab2.share_line_attack"),
            LabKind::Split => ("lab2.share_split", "lab2.share_line_split"),
        };
        ui.horizontal(|ui| {
            ui.label(RichText::new(text(voice, label)).color(t.text));
            // The same slider as the speed on Home, with a notch at half: past it, one side wins.
            let mut share = f32::from(choice.share);
            ui.allocate_ui(egui::vec2(SHARE_WIDTH, tokens::CONTROL), |ui| {
                slider(ui, t, &mut share, 10.0..=90.0, text(voice, label), &[0.5]);
            });
            // Clamped to the slider's range, so it fits a byte.
            choice.share = share.round().clamp(10.0, 90.0) as u8;
            let (mine, theirs) = (choice.share.to_string(), (100 - choice.share).to_string());
            ui.label(RichText::new(fill(text(voice, line), &[&mine, &theirs])).strong().color(t.text));
        });
        // One switch a line: side by side the two read as one sentence.
        if choice.kind == LabKind::Attack {
            switch_row(ui, t, &mut choice.helper, text(voice, "lab2.helper"));
        }
        switch_row(ui, t, &mut choice.keep, text(voice, "lab2.keep"));
        if voice.mode == Mode::Expert && choice.kind == LabKind::Attack {
            ui.horizontal(|ui| {
                ui.label(text(voice, "lab2.amount"));
                number_field(ui, &mut choice.amount, text(voice, "lab2.amount"), 70.0);
                ui.label(text(voice, "lab2.confirmations"));
                number_field(ui, &mut choice.confirmations, text(voice, "lab2.confirmations"), 60.0);
            });
            choice.amount = choice.amount.clamp(1, 1_000);
            choice.confirmations = choice.confirmations.clamp(1, 6);
        }
    });
}

fn buttons(ui: &mut Ui, t: &Tokens, voice: Voice, choice: &LabChoice, seen: &Seen) -> Option<Act> {
    let mut act = None;
    ui.horizontal(|ui| {
        let start = match choice.kind {
            LabKind::Attack => "lab2.start_attack",
            LabKind::Split => "lab2.start_split",
        };
        if button_on(ui, t, text(voice, start), Kind::Main, !seen.running).clicked() {
            act = Some(Act::Start);
        }
        if seen.running && button(ui, t, text(voice, "lab2.stop"), Kind::Danger).clicked() {
            act = Some(Act::Stop);
        }
        if seen.kept && !seen.running && button(ui, t, text(voice, "lab2.remove_kept"), Kind::Plain).clicked() {
            act = Some(Act::RemoveKept);
        }
    });
    act
}

/// The words under the buttons: what a run does, or the step it is at.
fn now(ui: &mut Ui, t: &Tokens, voice: Voice, choice: &LabChoice, seen: &Seen) {
    let about = match choice.kind {
        LabKind::Attack => "lab2.about_attack",
        LabKind::Split => "lab2.about_split",
    };
    explain::short(ui, t, text(voice, about));
    let words = match seen.state {
        Some(state) => now_words(voice, state),
        None if seen.kept => text(voice, "lab2.kept").to_string(),
        None => fill(text(voice, "lab2.idle_line"), &[&miners(choice).to_string()]),
    };
    let colour = match seen.state.map(|state| (state.phase, state.outcome)) {
        Some((Phase::Failed, _) | (Phase::Done, Some(Outcome::Gone))) => t.bad,
        Some((Phase::Done, _)) => t.good,
        _ => t.text,
    };
    ui.label(RichText::new(words).strong().color(colour).size(tokens::SECTION));
}

fn legend(ui: &mut Ui, t: &Tokens, voice: Voice, kind: LabKind) {
    let plain = |side| Mark { height: 0, id: String::new(), side, payment: false, spend: false };
    let (strong, other) = match kind {
        LabKind::Attack => ("lab2.legend_attacker", "lab2.legend_victim"),
        LabKind::Split => ("lab2.legend_left", "lab2.legend_right"),
    };
    ui.horizontal_wrapped(|ui| {
        lab_strip::swatch(ui, t, lab_strip::side_colour(t, kind, Side::Strong), &plain(Side::Strong), false, text(voice, strong));
        lab_strip::swatch(ui, t, lab_strip::side_colour(t, kind, Side::Other), &plain(Side::Other), false, text(voice, other));
        let shared = plain(Side::Other);
        lab_strip::swatch(ui, t, lab_strip::side_colour(t, kind, Side::Other), &shared, true, text(voice, "lab2.legend_shared"));
        if kind == LabKind::Attack {
            let paid = Mark { payment: true, ..plain(Side::Other) };
            lab_strip::swatch(ui, t, lab_strip::side_colour(t, kind, Side::Other), &paid, false, text(voice, "lab2.legend_payment"));
            let spent = Mark { spend: true, ..plain(Side::Strong) };
            lab_strip::swatch(ui, t, lab_strip::side_colour(t, kind, Side::Strong), &spent, false, text(voice, "lab2.legend_spend"));
        }
    });
}

/// Each step so far, numbered, with the chains as they stood.
fn timeline(ui: &mut Ui, t: &Tokens, voice: Voice, state: &LabState) {
    let kind = state.kind.unwrap_or(LabKind::Attack);
    section(ui, text(voice, "lab2.steps"));
    for (index, frame) in state.frames.iter().enumerate() {
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("{}", index + 1)).strong().color(t.muted).size(tokens::META));
            ui.vertical(|ui| {
                ui.label(RichText::new(step_words(voice, kind, &frame.step)).color(t.text));
                lab_strip::picture(ui, t, voice, kind, &frame.picture, SMALL);
            });
        });
        ui.add_space(4.0);
    }
}

/// The two experiments on the same four questions.
fn compare(ui: &mut Ui, voice: Voice, t: &Tokens) {
    section(ui, text(voice, "lab2.compare_title"));
    let say = |key| text(voice, key);
    let criteria = [say("lab2.compare_need"), say("lab2.compare_loss"), say("lab2.compare_time"), say("lab2.compare_defend")];
    let attack = [say("lab2.need_attack"), say("lab2.loss_attack"), say("lab2.time_attack"), say("lab2.defend_attack")];
    let split = [say("lab2.need_split"), say("lab2.loss_split"), say("lab2.time_split"), say("lab2.defend_split")];
    let choices = [
        Choice { name: say("lab2.kind_attack"), values: &attack },
        Choice { name: say("lab2.kind_split"), values: &split },
    ];
    explain::compare(ui, t, "lab2", &criteria, &choices);
}

/// Behind Details: the picture's legend first, then the analogy, each step with its chains, and
/// the two experiments on the same criteria.
fn details(ui: &mut Ui, t: &Tokens, voice: Voice, kind: LabKind, state: Option<&LabState>) {
    let analogy = match kind {
        LabKind::Attack => "lab2.analogy_attack",
        LabKind::Split => "lab2.analogy_split",
    };
    legend(ui, t, voice, kind);
    ui.add_space(tokens::GAP);
    explain::analogy(ui, t, text(voice, analogy));
    if let Some(state) = state.filter(|state| !state.frames.is_empty()) {
        ui.add_space(tokens::GAP_WIDE);
        timeline(ui, t, voice, state);
    }
    ui.add_space(tokens::GAP_WIDE);
    compare(ui, voice, t);
}

/// The Lab card. Returns what was pressed.
pub fn draw(ui: &mut Ui, t: &Tokens, voice: Voice, choice: &mut LabChoice, seen: &Seen) -> Option<Act> {
    card(ui, t, |ui| {
        choose(ui, t, voice, choice, seen.running);
        ui.add_space(tokens::GAP);
        let act = buttons(ui, t, voice, choice, seen);
        ui.add_space(tokens::GAP);
        let kind = seen.state.and_then(|state| state.kind).unwrap_or(choice.kind);
        let shown = Seen { state: seen.state.filter(|state| state.kind == Some(choice.kind)), ..*seen };
        now(ui, t, voice, choice, &shown);
        ui.add_space(tokens::GAP_WIDE);
        match shown.state {
            Some(state) => lab_strip::picture(ui, t, voice, kind, &state.live, LIVE),
            // Before a run, the two rows as they will start, from what is chosen above.
            None => lab_strip::picture(ui, t, voice, choice.kind, &lab_strip::planned(choice.kind, choice.share, choice.total_rate), LIVE),
        }
        ui.add_space(tokens::GAP);
        let labels = (text(voice, "explain.details"), text(voice, "explain.hide"));
        explain::details(ui, t, "lab2", labels, |ui| details(ui, t, voice, choice.kind, shown.state));
        act
    })
}
