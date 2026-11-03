//! How a feature, a setting or a choice explains itself. In place it says at most three short
//! lines. Everything else waits behind one Details control: a picture first, then one analogy if a
//! good one exists, then the same criteria for every option it is compared with, so similar choices
//! are judged by one stated measure and none is favoured by the wording.
use crate::ui::tokens::{self, Tokens};
use crate::ui::widgets::{button, card, cell, header_cell, meta, Kind};
use egui::{Grid, RichText, Ui};
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
    let mut open = ui.ctx().data(|data| data.get_temp::<bool>(id)).unwrap_or(false);
    let label = if open { labels.1 } else { labels.0 };
    if button(ui, t, label, Kind::Quiet).clicked() {
        open = !open;
        ui.ctx().data_mut(|data| data.insert_temp(id, open));
    }
    if open {
        ui.add_space(tokens::GAP);
        card(ui, t, body);
    }
}

/// One analogy, set apart from the facts around it.
pub fn analogy(ui: &mut Ui, t: &Tokens, text: &str) {
    ui.label(RichText::new(text).italics().color(t.soft));
}

/// Similar options on the same criteria: a row per criterion, a column per option.
pub fn compare(ui: &mut Ui, t: &Tokens, id: impl Hash + std::fmt::Debug, criteria: &[&str], choices: &[Choice]) {
    Grid::new(ui.make_persistent_id(("compare", id))).spacing([tokens::GAP_WIDE, tokens::GAP]).show(ui, |ui| {
        header_cell(ui, t, "");
        for choice in choices {
            header_cell(ui, t, choice.name);
        }
        ui.end_row();
        for (row, criterion) in criteria.iter().enumerate() {
            meta(ui, criterion, t);
            for choice in choices {
                cell(ui, choice.values.get(row).copied().unwrap_or_default());
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
