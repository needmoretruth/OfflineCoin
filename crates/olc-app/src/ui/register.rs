//! Settings as a register: name, current value and control, and what it means, in three columns.
use super::paint::fading_line;
use super::tokens::{self, Tokens};
use super::widgets::section;
use egui::{vec2, Align, Label, Layout, RichText, Ui};

const NAME: f32 = 0.26;
const VALUE: f32 = 0.36;
const SINGLE_ROW: f32 = 46.0;

/// Starts a group of rows.
pub fn group(ui: &mut Ui, heading: &str) {
    ui.add_space(tokens::GAP_WIDE);
    section(ui, heading);
    ui.add_space(4.0);
}

/// One line of the register. `control` draws the current value, and the control if there is one.
pub fn row(ui: &mut Ui, t: &Tokens, name: &str, meaning: &str, control: impl FnOnce(&mut Ui)) {
    let width = ui.available_width();
    let top = ui.cursor().top();
    let left = ui.cursor().left();
    ui.horizontal_top(|ui| {
        ui.set_min_height(SINGLE_ROW);
        let name_width = (width * NAME).round();
        let value_width = (width * VALUE).round();
        let height = SINGLE_ROW;
        ui.allocate_ui_with_layout(vec2(name_width, height), Layout::left_to_right(Align::Center), |ui| {
            ui.set_width(name_width - tokens::GAP);
            ui.set_min_height(height);
            ui.add(Label::new(RichText::new(name).color(t.text)).wrap());
        });
        ui.allocate_ui_with_layout(vec2(value_width, height), Layout::left_to_right(Align::Center).with_main_wrap(true), |ui| {
            ui.set_width(value_width - tokens::GAP);
            ui.set_min_height(height);
            control(ui);
        });
        ui.add_space((left + name_width + value_width - ui.cursor().left()).max(0.0));
        ui.allocate_ui_with_layout(vec2(width - name_width - value_width - 16.0, height), Layout::left_to_right(Align::Center), |ui| {
            ui.set_min_height(height);
            let text = RichText::new(meaning).size(tokens::META).color(t.muted);
            ui.add(Label::new(text).wrap());
        });
    });
    let rect = ui.min_rect();
    let bottom = ui.cursor().top().max(top + SINGLE_ROW);
    fading_line(ui.painter(), rect.left(), rect.left() + width, bottom - 2.0, t.line);
}
