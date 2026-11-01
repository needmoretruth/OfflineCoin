//! The controls every page uses, so a page says what it shows and not how it is drawn.
use super::fonts::strong;
use super::paint::{surface, Surface};
use super::tokens::{self, Tokens};
use egui::epaint::Shadow;
use egui::{
    pos2, vec2, Align, Color32, CornerRadius, FontFamily, FontId, Frame, Galley, Label, Layout,
    Margin, Rect, Response, RichText, Sense, Stroke, StrokeKind, TextEdit, TextStyle, Ui, Vec2,
    WidgetText,
};
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Main,
    Plain,
    Quiet,
    Danger,
}

fn focus_ring(ui: &Ui, rect: Rect, response: &Response, t: &Tokens) {
    if response.has_focus() {
        ui.painter().rect_stroke(
            rect.expand(2.0),
            CornerRadius::same(tokens::RADIUS_CONTROL + 1),
            Stroke::new(1.5, t.info),
            StrokeKind::Outside,
        );
    }
}

fn layout(ui: &Ui, text: &str, font: FontId, colour: Color32) -> Arc<Galley> {
    ui.painter().layout_no_wrap(text.to_string(), font, colour)
}

/// Text centred in a box of the given height, however the font sits in its line.
fn paint_centred(ui: &Ui, rect: Rect, galley: Arc<Galley>, colour: Color32) {
    let at = rect.center() - galley.size() / 2.0;
    ui.painter().galley(pos2(at.x, at.y), galley, colour);
}

pub fn button(ui: &mut Ui, t: &Tokens, text: &str, kind: Kind) -> Response {
    button_on(ui, t, text, kind, true)
}

pub fn button_on(ui: &mut Ui, t: &Tokens, text: &str, kind: Kind, enabled: bool) -> Response {
    let font = TextStyle::Button.resolve(ui.style());
    let mut galley = layout(ui, text, font.clone(), t.text);
    let width = galley.size().x + 28.0;
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, response) = ui.allocate_exact_size(vec2(width, tokens::CONTROL), sense);
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let down = response.is_pointer_button_down_on();
    let hover = response.hovered() && enabled;
    let (top, bottom, line, colour) = match (kind, enabled) {
        (_, false) => (t.raised, t.raised, t.line, t.muted),
        (Kind::Main, true) => (t.main_top, t.main_bottom, t.main_bottom, t.main_text),
        (Kind::Plain, true) => {
            let fill = if hover { t.hover } else { t.raised };
            (fill, fill, if hover { t.line_strong } else { t.line }, t.text)
        }
        (Kind::Quiet, true) => {
            let fill = if hover { t.hover } else { Color32::TRANSPARENT };
            (fill, fill, Color32::TRANSPARENT, if hover { t.text } else { t.muted })
        }
        (Kind::Danger, true) => {
            let fill = if hover { t.hover } else { t.raised };
            (fill, fill, t.line, t.bad)
        }
    };
    let glint = if kind == Kind::Plain && enabled { t.glint } else { Color32::TRANSPARENT };
    let area = if down { rect.translate(vec2(0.0, 1.0)) } else { rect };
    let look = Surface {
        top,
        bottom,
        line,
        glint,
        shadow: None,
        radius: tokens::RADIUS_CONTROL,
    };
    surface(ui.painter(), area, &look, None);
    galley = layout(ui, text, font, colour);
    paint_centred(ui, area, galley, colour);
    focus_ring(ui, rect, &response, t);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, text));
    if enabled {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    }
}

pub fn chip(ui: &mut Ui, text: &str, colour: Color32, t: &Tokens) -> Response {
    let font = FontId::new(tokens::META, FontFamily::Proportional);
    let galley = layout(ui, text, font, colour);
    let size = vec2(galley.size().x + 12.0, 22.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    let round = CornerRadius::same(tokens::RADIUS_CHIP);
    ui.painter().rect_filled(rect, round, t.raised);
    paint_centred(ui, rect, galley, colour);
    response
}

pub fn dot(ui: &mut Ui, colour: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
    ui.painter().circle_filled(rect.center(), 4.0, colour);
}

pub fn card<R>(ui: &mut Ui, t: &Tokens, add: impl FnOnce(&mut Ui) -> R) -> R {
    let slot = ui.painter().add(egui::Shape::Noop);
    let margin = Margin::same(tokens::CARD_MARGIN);
    let shown = Frame::new().inner_margin(margin).show(ui, |ui| {
        ui.set_width(ui.available_width());
        add(ui)
    });
    let look = Surface {
        top: t.card_top,
        bottom: t.card_bottom,
        line: t.line,
        glint: t.glint,
        shadow: Some(Shadow {
            offset: [0, 2],
            blur: 10,
            spread: 0,
            color: t.shadow,
        }),
        radius: tokens::RADIUS_CARD,
    };
    surface(ui.painter(), shown.response.rect, &look, Some(slot));
    shown.inner
}

pub fn title(ui: &mut Ui, text: &str) {
    ui.add(Label::new(RichText::new(text).heading()).selectable(false));
    ui.add_space(tokens::GAP);
}

pub fn section(ui: &mut Ui, text: &str) {
    let text = RichText::new(text)
        .family(strong())
        .size(tokens::SECTION);
    ui.add(Label::new(text).selectable(false));
}

pub fn meta(ui: &mut Ui, text: &str, t: &Tokens) -> Response {
    let text = RichText::new(text).size(tokens::META).color(t.muted);
    ui.add(Label::new(text).wrap())
}

pub fn strong_text(text: impl Into<String>) -> RichText {
    RichText::new(text.into()).family(strong())
}

pub fn mono(text: impl Into<String>) -> RichText {
    RichText::new(text.into()).monospace()
}

/// A figure with its name under it.
pub fn stat(ui: &mut Ui, t: &Tokens, name: &str, value: &str) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        ui.add(Label::new(strong_text(value).size(tokens::TITLE)).truncate());
        ui.add(Label::new(RichText::new(name).size(tokens::META).color(t.muted)).truncate());
    });
}

/// A row of figures sharing the width.
pub fn stats(ui: &mut Ui, t: &Tokens, items: &[(&str, String)]) {
    let width = ui.available_width() / items.len().max(1) as f32 - tokens::GAP;
    ui.horizontal(|ui| {
        for (name, value) in items {
            ui.allocate_ui_with_layout(vec2(width, 52.0), Layout::top_down(Align::Min), |ui| {
                ui.set_width(width);
                stat(ui, t, name, value);
            });
        }
    });
}

pub fn field(ui: &mut Ui, text: &mut String, hint: &str, width: f32) -> Response {
    let edit = TextEdit::singleline(text)
        .hint_text(hint)
        .desired_width(width)
        .margin(vec2(10.0, 8.0))
        .align(egui::Align2::LEFT_CENTER)
        .min_size(vec2(width, tokens::CONTROL));
    ui.add(edit)
}

/// A field whose text stays hidden, for secret keys and passwords.
pub fn secret_field(ui: &mut Ui, text: &mut String, hint: &str, width: f32) -> Response {
    let edit = TextEdit::singleline(text)
        .password(true)
        .hint_text(hint)
        .desired_width(width)
        .margin(vec2(10.0, 8.0))
        .align(egui::Align2::LEFT_CENTER)
        .min_size(vec2(width, tokens::CONTROL));
    ui.add(edit)
}

/// A number typed as text and taken when it reads as one. Returns the new value once.
pub fn number_field(ui: &mut Ui, value: &mut u64, width: f32) -> bool {
    let id = ui.next_auto_id().with("typed");
    let mut typed: String = ui
        .data_mut(|data| data.get_temp(id))
        .unwrap_or_else(|| value.to_string());
    let response = field(ui, &mut typed, "", width);
    let mut changed = false;
    if response.changed() {
        if let Ok(number) = typed.trim().parse::<u64>() {
            changed = number != *value;
            *value = number;
        }
    }
    if response.has_focus() {
        ui.data_mut(|data| data.insert_temp(id, typed));
    } else {
        ui.data_mut(|data| data.remove_temp::<String>(id));
    }
    changed
}

pub fn switch(ui: &mut Ui, t: &Tokens, on: &mut bool, label: &str) -> Response {
    let size = vec2(36.0, 20.0);
    let (rect, mut response) = ui.allocate_exact_size(size, Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    let how_far = ui.ctx().animate_bool(response.id, *on);
    let radius = rect.height() / 2.0;
    let fill = if *on { t.good } else { t.line_strong };
    ui.painter().rect_filled(rect, CornerRadius::same(10), fill);
    let x = egui::lerp(rect.left() + radius..=rect.right() - radius, how_far);
    let knob = if t.dark { t.text } else { Color32::WHITE };
    ui.painter().circle_filled(pos2(x, rect.center().y), radius - 3.0, knob);
    focus_ring(ui, rect, &response, t);
    let state = *on;
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::Role::Switch, true, state, label)
    });
    response
}

/// A label with a switch at its left.
pub fn switch_row(ui: &mut Ui, t: &Tokens, on: &mut bool, label: &str) -> bool {
    ui.horizontal(|ui| {
        let changed = switch(ui, t, on, label).changed();
        ui.add(Label::new(label).selectable(false));
        changed
    })
    .inner
}

/// Choices side by side, one of them current. Returns the one clicked.
pub fn segments<T: Copy + PartialEq>(
    ui: &mut Ui,
    t: &Tokens,
    items: &[(T, String)],
    current: Option<T>,
) -> Option<T> {
    let mut chosen = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        for (value, label) in items {
            let kind = if current == Some(*value) { Kind::Main } else { Kind::Plain };
            if button(ui, t, label, kind).clicked() {
                chosen = Some(*value);
            }
        }
    });
    chosen
}

/// A short message where a list would be, saying what is true and what to do.
pub fn empty(ui: &mut Ui, t: &Tokens, heading: &str, body: &str) {
    card(ui, t, |ui| {
        ui.add_space(tokens::GAP_WIDE);
        ui.vertical_centered(|ui| {
            section(ui, heading);
            ui.add_space(2.0);
            meta(ui, body, t);
        });
        ui.add_space(tokens::GAP_WIDE);
    });
}

/// Column widths from weights, for rows that line up.
pub struct Columns {
    widths: Vec<f32>,
}

impl Columns {
    pub fn new(ui: &Ui, weights: &[f32]) -> Self {
        let gap = tokens::GAP * (weights.len().saturating_sub(1)) as f32;
        let space = (ui.available_width() - gap).max(1.0);
        let total: f32 = weights.iter().sum();
        Self {
            widths: weights.iter().map(|weight| weight / total * space).collect(),
        }
    }

    /// One row. `cell` is given each column in turn.
    pub fn row(&self, ui: &mut Ui, mut cell: impl FnMut(&mut Ui, usize)) {
        ui.horizontal(|ui| {
            let height = tokens::ROW - ui.spacing().item_spacing.y;
            ui.set_min_height(height);
            for (index, width) in self.widths.iter().enumerate() {
                ui.allocate_ui_with_layout(
                    vec2(*width, height),
                    Layout::left_to_right(Align::Center),
                    |ui| {
                        ui.set_width(*width);
                        cell(ui, index);
                    },
                );
            }
        });
    }
}

/// Text cut to its column.
pub fn cell(ui: &mut Ui, text: impl Into<WidgetText>) {
    ui.add(Label::new(text).truncate());
}

pub fn header_cell(ui: &mut Ui, t: &Tokens, text: &str) {
    let text = RichText::new(text).size(tokens::META).color(t.muted);
    ui.add(Label::new(text).truncate().selectable(false));
}

pub fn divider(ui: &mut Ui, t: &Tokens) {
    let rect = ui.available_rect_before_wrap();
    let y = rect.top();
    ui.painter().hline(rect.x_range(), y, Stroke::new(1.0, t.line));
    ui.add_space(1.0);
}
