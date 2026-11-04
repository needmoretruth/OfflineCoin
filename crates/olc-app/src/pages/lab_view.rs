//! The experiment card of the Lab: an attack or a split running on the network's own
//! participants. At most three lines of words, the two chains as they race, and behind "Details"
//! each step with its picture, a legend, an analogy and the two experiments side by side. Drawing
//! only: it takes words and a state and gives back what was pressed, so a screen test can draw any
//! state of a real run.
use super::lab_strip::{self, LIVE, SMALL};
use super::lab_words::{now_words, step_words};
use crate::i18n::{text, Voice};
use crate::local::lab::{LabKind, LabState, Mark, Phase, Side};
use crate::ui::tokens::{self, Tokens};
use crate::ui::explain::{self, Choice};
use crate::ui::widgets::{button, card, section, Kind};
use egui::{RichText, Ui};
use olc_lab::Outcome;

/// What a press on the experiment card asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Act {
    Stop,
    Close,
}

/// The words under the title: what the experiment does, and the step it is at.
fn now(ui: &mut Ui, t: &Tokens, voice: Voice, state: &LabState) {
    let kind = state.kind.unwrap_or(LabKind::Attack);
    let about = match kind {
        LabKind::Attack => "lab2.about_attack",
        LabKind::Split => "lab2.about_split",
    };
    explain::short(ui, t, text(voice, about));
    let colour = match (state.phase, state.outcome) {
        (Phase::Failed, _) | (Phase::Done, Some(Outcome::Gone)) => t.bad,
        (Phase::Done, _) => t.good,
        _ => t.text,
    };
    ui.label(RichText::new(now_words(voice, state)).strong().color(colour).size(tokens::SECTION));
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

/// The card of the experiment running or last run. Returns what was pressed.
pub fn draw(ui: &mut Ui, t: &Tokens, voice: Voice, state: &LabState) -> Option<Act> {
    let kind = state.kind.unwrap_or(LabKind::Attack);
    card(ui, t, |ui| {
        let mut act = None;
        ui.horizontal(|ui| {
            let title = match kind {
                LabKind::Attack => "lab2.kind_attack",
                LabKind::Split => "lab2.kind_split",
            };
            section(ui, text(voice, title));
            let running = matches!(state.phase, Phase::Starting | Phase::Warming | Phase::Running);
            let (key, pressed) = if running { ("lab2.stop", Act::Stop) } else { ("explain.hide", Act::Close) };
            if button(ui, t, text(voice, key), Kind::Quiet).clicked() {
                act = Some(pressed);
            }
        });
        now(ui, t, voice, state);
        ui.add_space(tokens::GAP_WIDE);
        lab_strip::picture(ui, t, voice, kind, &state.live, LIVE);
        ui.add_space(tokens::GAP);
        let labels = (text(voice, "explain.details"), text(voice, "explain.hide"));
        explain::details(ui, t, "lab2", labels, |ui| details(ui, t, voice, kind, Some(state)));
        act
    })
}
