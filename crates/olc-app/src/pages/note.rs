//! The developer's note, in the developer's own words.
use super::Cx;
use crate::ui::floating::sheet_head;
use crate::ui::tokens;
use crate::ui::widgets::{button, Kind};
use egui::{Label, RichText, ScrollArea, Ui};

/// The note is long, and the sheet must leave its close button on screen.
const MOST_OF_THE_WINDOW: f32 = 0.66;

/// Draws the note. Returns true when the person closes it.
pub fn show(ui: &mut Ui, cx: &Cx) -> bool {
    let t = cx.t;
    sheet_head(ui, cx.say("note.title"));
    let height = ui.ctx().content_rect().height() * MOST_OF_THE_WINDOW;
    ScrollArea::vertical().max_height(height).show(ui, |ui| {
        for paragraph in cx.say("note.body").split("\n\n") {
            ui.add(Label::new(RichText::new(paragraph).color(t.text)).wrap());
            ui.add_space(tokens::GAP);
        }
    });
    button(ui, t, cx.say("common.close"), Kind::Plain).clicked()
}
