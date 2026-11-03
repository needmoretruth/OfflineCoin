//! Settings as a register: name, current value and control, and what it means, in three columns.
use super::explain;
use super::paint::fading_line;
use super::tokens::{self, Tokens};
use super::widgets::section;
use egui::{vec2, Align, Color32, Id, Label, Layout, RichText, Ui};

const NAME: f32 = 0.26;
const VALUE: f32 = 0.36;
const SINGLE_ROW: f32 = 46.0;
/// Room between a meaning that fills its line and the line under it.
const OVER_ROW: f32 = 6.0;

/// Starts a group of rows.
pub fn group(ui: &mut Ui, heading: &str) {
    ui.add_space(tokens::GAP_WIDE);
    section(ui, heading);
    ui.add_space(tokens::GAP);
}

/// When a setting takes hold, said under its name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum When {
    Now,
    NextStart,
}

/// One line of the register. `control` draws the current value, and the control if there is one.
pub fn row(ui: &mut Ui, t: &Tokens, name: &str, meaning: &str, control: impl FnOnce(&mut Ui)) {
    row_with(ui, t, (name, None), |ui| meaning_line(ui, t, meaning), control);
}

/// A setting's line. One that waits for the next start says so under its name, in `words` and in
/// the colour of a warning, since nothing on the screen moves yet; the page says once that the
/// rest apply at once.
pub fn setting(ui: &mut Ui, t: &Tokens, name: &str, (when, words): (When, &str), meaning: &str, control: impl FnOnce(&mut Ui)) {
    row_with(ui, t, (name, tag(t, when, words)), |ui| meaning_line(ui, t, meaning), control);
}

fn tag<'a>(t: &Tokens, when: When, words: &'a str) -> Option<(&'a str, Color32)> {
    (when == When::NextStart).then_some((words, t.warn))
}

/// A setting whose comparison waits behind Details. The control stands under what the setting
/// means, and what it opens spans the whole line under it, so it reads as this setting's.
pub fn setting_details(
    ui: &mut Ui,
    t: &Tokens,
    (name, (when, words)): (&str, (When, &str)),
    (meaning, key, labels): (&str, &str, (&str, &str)),
    control: impl FnOnce(&mut Ui),
    body: impl FnOnce(&mut Ui),
) {
    let id = Id::unique(("setting-details", key));
    let mut open = false;
    let meaning_and_more = |ui: &mut Ui| {
        ui.vertical(|ui| {
            meaning_line(ui, t, meaning);
            open = explain::toggle(ui, t, id, labels);
        });
    };
    row_with(ui, t, (name, tag(t, when, words)), meaning_and_more, control);
    if open {
        explain::opened(ui, t, body);
        ui.add_space(tokens::GAP_WIDE);
    }
}

fn meaning_line(ui: &mut Ui, t: &Tokens, meaning: &str) {
    let text = RichText::new(meaning).size(tokens::META).color(t.muted);
    ui.add(Label::new(text).wrap());
}

type Name<'a> = (&'a str, Option<(&'a str, Color32)>);

fn row_with(ui: &mut Ui, t: &Tokens, (name, tag): Name, meaning: impl FnOnce(&mut Ui), control: impl FnOnce(&mut Ui)) {
    let width = ui.available_width();
    let top = ui.cursor().top();
    let left = ui.cursor().left();
    let said = ui.horizontal_top(|ui| {
        ui.set_min_height(SINGLE_ROW);
        let name_width = (width * NAME).round();
        let value_width = (width * VALUE).round();
        let height = SINGLE_ROW;
        ui.allocate_ui_with_layout(vec2(name_width, height), Layout::left_to_right(Align::Center), |ui| {
            ui.set_width(name_width - tokens::GAP);
            ui.set_min_height(height);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.add(Label::new(RichText::new(name).color(t.text)).wrap());
                if let Some((words, colour)) = tag {
                    ui.add(Label::new(RichText::new(words).size(tokens::META).color(colour)).wrap());
                }
            });
        });
        ui.allocate_ui_with_layout(vec2(value_width, height), Layout::left_to_right(Align::Center).with_main_wrap(true), |ui| {
            ui.set_width(value_width - tokens::GAP);
            ui.set_min_height(height);
            control(ui);
        });
        ui.add_space((left + name_width + value_width - ui.cursor().left()).max(0.0));
        let room = vec2(width - name_width - value_width - 16.0, height);
        let column = ui.allocate_ui_with_layout(room, Layout::left_to_right(Align::Center), |ui| {
            ui.set_min_height(height);
            ui.scope(meaning).response.rect
        });
        column.inner
    });
    // Three lines of meaning, or one and a Details control, fill the line to its edges.
    if said.inner.height() > SINGLE_ROW - OVER_ROW {
        ui.add_space(OVER_ROW);
    }
    let rect = ui.min_rect();
    let bottom = ui.cursor().top().max(top + SINGLE_ROW);
    fading_line(ui.painter(), rect.left(), rect.left() + width, bottom - 2.0, t.line);
}
