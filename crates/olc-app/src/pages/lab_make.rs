//! Makes blocks on the Lab's own coin now, as many as a person types: the chains below grow while
//! it runs, and the next experiment starts from these blocks.
use super::lab_words::now_words;
use crate::i18n::{text, Voice};
use crate::local::lab::{LabState, Phase, MOST_BLOCKS};
use crate::ui::explain;
use crate::ui::tokens::{self, Tokens};
use crate::ui::widgets::{button_on, card, number_field, section, Kind};
use egui::{RichText, Ui};

const COUNT_WIDTH: f32 = 80.0;

/// How many blocks a press makes, kept between frames.
pub struct Ask {
    pub count: u64,
}

impl Default for Ask {
    fn default() -> Self {
        Self { count: 10 }
    }
}

/// The card. Returns the number of blocks to make when the button was pressed.
pub fn draw(ui: &mut Ui, t: &Tokens, voice: Voice, ask: &mut Ask, state: Option<&LabState>, running: bool) -> Option<u64> {
    card(ui, t, |ui| {
        section(ui, text(voice, "lab2.make_title"));
        explain::short(ui, t, text(voice, "lab2.make_about"));
        ui.add_space(tokens::GAP);
        let mut pressed = None;
        ui.horizontal(|ui| {
            ui.label(text(voice, "lab2.make_count"));
            ui.add_enabled_ui(!running, |ui| number_field(ui, &mut ask.count, text(voice, "lab2.make_count"), COUNT_WIDTH));
            ask.count = ask.count.clamp(1, MOST_BLOCKS);
            if button_on(ui, t, text(voice, "lab2.make"), Kind::Main, !running).clicked() {
                pressed = Some(ask.count);
            }
        });
        if let Some(state) = state.filter(|state| state.making.is_some()) {
            let colour = match state.phase {
                Phase::Failed => t.bad,
                Phase::Done => t.good,
                _ => t.text,
            };
            ui.add_space(tokens::GAP);
            ui.label(RichText::new(now_words(voice, state)).strong().color(colour));
        }
        let labels = (text(voice, "explain.details"), text(voice, "explain.hide"));
        explain::details(ui, t, "lab2-make", labels, |ui| {
            ui.label(text(voice, "lab2.make_details"));
        });
        pressed
    })
}
