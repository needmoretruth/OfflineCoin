//! How a feature, a setting or a choice explains itself. In place it says at most three short
//! lines. Everything else waits behind one Details control: a picture first, then one analogy if a
//! good one exists, then the same criteria for every option it is compared with, so similar choices
//! are judged by one stated measure and none is favoured by the wording.
use crate::ui::tokens::{self, Tokens};
use crate::ui::widgets::{button, card, meta, Kind};
use egui::{Grid, Id, Label, RichText, Ui};
use std::hash::Hash;

/// The widest text that still reads as three lines in place, in Latin characters. A Hangul
/// syllable is about twice as wide as a Latin character at the app's text size.
pub const MOST_WIDTH: usize = 220;

/// One option among similar ones, with one value for each criterion, in the criteria's order.
pub struct Choice<'a> {
    pub name: &'a str,
    pub values: &'a [&'a str],
}

/// True when `text` fits the three lines an explanation may take in place.
pub fn fits_in_place(text: &str) -> bool {
    let hangul = text.chars().filter(|c| ('\u{AC00}'..='\u{D7A3}').contains(c)).count();
    let other = text.chars().count() - hangul;
    hangul * 2 + other <= MOST_WIDTH
}

/// The short text that stays in place.
pub fn short(ui: &mut Ui, t: &Tokens, text: &str) {
    debug_assert!(fits_in_place(text), "longer than three lines in place: {text}");
    meta(ui, text, t);
}

/// A Details control. `body` opens below it in place and closes with the same control. The open
/// state lives with the id, so a page that redraws keeps it.
pub fn details(ui: &mut Ui, t: &Tokens, id: impl Hash + std::fmt::Debug, labels: (&str, &str), body: impl FnOnce(&mut Ui)) {
    let id = ui.make_persistent_id(("details", id));
    if toggle(ui, t, id, labels) {
        opened(ui, t, body);
    }
}

/// The Details control alone, for a place where what opens goes somewhere else, as under a whole
/// setting's line. Returns true while it is open.
pub fn toggle(ui: &mut Ui, t: &Tokens, id: Id, labels: (&str, &str)) -> bool {
    let mut open = ui.ctx().data(|data| data.get_temp::<bool>(id)).unwrap_or(false);
    let label = if open { labels.1 } else { labels.0 };
    if button(ui, t, label, Kind::Quiet).clicked() {
        open = !open;
        ui.ctx().data_mut(|data| data.insert_temp(id, open));
    }
    open
}

/// What a Details control opens, in its card.
pub fn opened(ui: &mut Ui, t: &Tokens, body: impl FnOnce(&mut Ui)) {
    ui.add_space(tokens::GAP);
    card(ui, t, body);
}

/// One analogy, set apart from the facts around it.
pub fn analogy(ui: &mut Ui, t: &Tokens, text: &str) {
    ui.label(RichText::new(text).italics().color(t.soft));
}

/// The widest a column of the comparison grows, so a long value wraps instead of stretching it.
const COLUMN_MOST: f32 = 320.0;

/// Similar options on the same criteria: a row per criterion, a column per option. Every column
/// has one width and its text wraps: a cut cell would hide the very value being compared.
pub fn compare(ui: &mut Ui, t: &Tokens, id: impl Hash + std::fmt::Debug, criteria: &[&str], choices: &[Choice]) {
    let columns = choices.len() as f32 + 1.0;
    let width = ((ui.available_width() - tokens::GAP_WIDE * (columns - 1.0)) / columns).clamp(80.0, COLUMN_MOST);
    let wrapped = |ui: &mut Ui, text: RichText| {
        ui.set_width(width);
        ui.add(Label::new(text).wrap());
    };
    Grid::new(ui.make_persistent_id(("compare", id)))
        .spacing([tokens::GAP_WIDE, tokens::GAP])
        .min_col_width(width)
        .max_col_width(width)
        .show(ui, |ui| {
            ui.label("");
            for choice in choices {
                ui.scope(|ui| wrapped(ui, RichText::new(choice.name).size(tokens::META).color(t.muted)));
            }
            ui.end_row();
            for (row, criterion) in criteria.iter().enumerate() {
                ui.scope(|ui| wrapped(ui, RichText::new(*criterion).size(tokens::META).color(t.soft)));
                for choice in choices {
                    // A line break starts a smaller note under the value, as a measured number
                    // under the one to expect.
                    let value = choice.values.get(row).copied().unwrap_or_default();
                    match value.split_once('\n') {
                        Some((value, note)) => ui.vertical(|ui| {
                            wrapped(ui, RichText::new(value).color(t.text));
                            wrapped(ui, RichText::new(note).size(tokens::META).color(t.soft));
                        }),
                        None => ui.scope(|ui| wrapped(ui, RichText::new(value).color(t.text))),
                    };
                }
                ui.end_row();
            }
        });
}

#[cfg(test)]
mod tests {
    use super::fits_in_place;

    #[test]
    fn three_lines_hold_two_hundred_twenty_latin_or_one_hundred_ten_hangul() {
        assert!(fits_in_place(&"a".repeat(220)));
        assert!(!fits_in_place(&"a".repeat(221)));
        assert!(fits_in_place(&"가".repeat(110)));
        assert!(!fits_in_place(&"가".repeat(111)));
    }
}
