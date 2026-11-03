//! The controls every page uses, so a page says what it shows and not how it is drawn.
use super::fonts::{medium, strong};
use super::paint::{fading_line, surface, Surface};
use super::tokens::{self, Tokens};
use egui::{
    pos2, vec2, Align, Color32, CornerRadius, FontFamily, FontId, Frame, Galley, Label, Layout,
    Margin, Rect, Response, RichText, Sense, Stroke, StrokeKind, TextEdit, TextStyle, Ui, Vec2,
    WidgetText,
};
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// The one thing that moves the screen forward.
    Main,
    Plain,
    Quiet,
    /// Cannot be undone.
    Danger,
}

fn focus_ring(ui: &Ui, rect: Rect, response: &Response, t: &Tokens) {
    if response.has_focus() {
        ui.painter().rect_stroke(
            rect.expand(2.0),
            CornerRadius::same(tokens::RADIUS_BUTTON + 1),
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

/// The face of a button at its current press.
fn pressed_area(ui: &Ui, response: &Response, rect: Rect) -> Rect {
    let down = response.is_pointer_button_down_on();
    let id = response.id.with("press");
    let how_far = ui.ctx().animate_bool_with_time(id, down, tokens::PRESS_SECONDS);
    let scale = 1.0 - (1.0 - tokens::PRESS_SCALE) * how_far;
    Rect::from_center_size(rect.center(), rect.size() * scale)
}

pub fn button(ui: &mut Ui, t: &Tokens, text: &str, kind: Kind) -> Response {
    button_on(ui, t, text, kind, true)
}

/// How wide [`button`] draws a button with this text, so a line beside it can leave the room.
pub fn button_width(ui: &Ui, text: &str) -> f32 {
    let font = TextStyle::Button.resolve(ui.style());
    layout(ui, text, font, Color32::PLACEHOLDER).size().x + 28.0
}

pub fn button_on(ui: &mut Ui, t: &Tokens, text: &str, kind: Kind, enabled: bool) -> Response {
    let font = TextStyle::Button.resolve(ui.style());
    let width = button_width(ui, text);
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, response) = ui.allocate_exact_size(vec2(width, tokens::CONTROL), sense);
    // Named before the visibility check, or a button below the fold has no name for a screen reader.
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, text));
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let hover = response.hovered() && enabled;
    let clear = Color32::TRANSPARENT;
    let (top, bottom, line, glint, colour) = match (kind, enabled) {
        (_, false) => (clear, clear, t.line, clear, t.muted),
        (Kind::Main, true) => (t.main_top, t.main_bottom, clear, clear, t.main_text),
        (Kind::Danger, true) => (t.danger_top, t.danger_bottom, clear, clear, t.danger_text),
        (Kind::Plain, true) => {
            let edge = if hover { t.line_strong } else { t.line };
            (t.card_top, t.card_bottom, edge, t.glint, t.text)
        }
        (Kind::Quiet, true) => {
            let wash = if hover { t.hover } else { clear };
            (wash, wash, clear, clear, if hover { t.text } else { t.soft })
        }
    };
    let area = pressed_area(ui, &response, rect);
    let look = Surface { top, bottom, line, glint, shadow: None, radius: tokens::RADIUS_BUTTON };
    surface(ui.painter(), area, &look, None);
    if hover && matches!(kind, Kind::Main | Kind::Danger) {
        let lift = CornerRadius::same(tokens::RADIUS_BUTTON);
        ui.painter().rect_filled(area, lift, t.hover);
    }
    paint_centred(ui, area, layout(ui, text, font, colour), colour);
    focus_ring(ui, rect, &response, t);
    if enabled {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    }
}

/// A short state word in a small box.
pub fn tag(ui: &mut Ui, text: &str, colour: Color32, t: &Tokens) -> Response {
    let font = FontId::new(tokens::META, FontFamily::Proportional);
    let galley = layout(ui, text, font, colour);
    let size = vec2(galley.size().x + 12.0, 22.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter().rect_stroke(
        rect,
        CornerRadius::same(tokens::RADIUS_TAG),
        Stroke::new(1.0, t.line_strong),
        StrokeKind::Inside,
    );
    paint_centred(ui, rect, galley, colour);
    response
}

/// A state as a small light with a faint glow around it.
pub fn dot(ui: &mut Ui, colour: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(12.0), Sense::hover());
    ui.painter().circle_filled(rect.center(), 6.0, colour.gamma_multiply(0.18));
    ui.painter().circle_filled(rect.center(), 3.5, colour);
}

pub fn card<R>(ui: &mut Ui, t: &Tokens, add: impl FnOnce(&mut Ui) -> R) -> R {
    card_with_margin(ui, t, tokens::CARD_MARGIN, add)
}

pub fn card_with_margin<R>(ui: &mut Ui, t: &Tokens, margin: i8, add: impl FnOnce(&mut Ui) -> R) -> R {
    let slot = ui.painter().add(egui::Shape::Noop);
    let shown = Frame::new().inner_margin(Margin::same(margin)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        add(ui)
    });
    let look = Surface {
        top: t.card_top,
        bottom: t.card_bottom,
        line: t.line,
        glint: t.glint,
        shadow: None,
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
    let text = RichText::new(text).family(strong()).size(tokens::SECTION);
    ui.add(Label::new(text).selectable(false));
}

pub fn meta(ui: &mut Ui, text: &str, t: &Tokens) -> Response {
    let text = RichText::new(text).size(tokens::META).color(t.muted);
    ui.add(Label::new(text).wrap())
}

pub fn strong_text(text: impl Into<String>) -> RichText {
    RichText::new(text.into()).family(strong())
}

pub fn medium_text(text: impl Into<String>) -> RichText {
    RichText::new(text.into()).family(medium())
}

pub fn mono(text: impl Into<String>) -> RichText {
    RichText::new(text.into()).monospace()
}

fn input<'a>(text: &'a mut String, hint: &'a str, width: f32) -> TextEdit<'a> {
    TextEdit::singleline(text)
        .hint_text(hint)
        .desired_width(width)
        .margin(vec2(10.0, 8.0))
        .align(egui::Align2::LEFT_CENTER)
        .min_size(vec2(width, tokens::CONTROL))
}

/// `hint` also names the field for a screen reader, which reads it once the field is filled in.
pub fn field(ui: &mut Ui, text: &mut String, hint: &str, width: f32) -> Response {
    ui.add(input(text, hint, width)).accessible_name(hint)
}

/// A field for words of a script: fixed-width letters, and room for a few lines.
pub fn words_box(ui: &mut Ui, text: &mut String, hint: &str, width: f32) -> Response {
    let edit = TextEdit::multiline(text)
        .hint_text(hint)
        .code_editor()
        .desired_width(width)
        .desired_rows(3)
        .margin(vec2(10.0, 8.0));
    ui.add(edit).accessible_name(hint)
}

/// A field whose text stays hidden, for secret keys and passwords.
pub fn secret_field(ui: &mut Ui, text: &mut String, hint: &str, width: f32) -> Response {
    ui.add(input(text, hint, width).password(true)).accessible_name(hint)
}

/// A number typed as text and taken when it reads as one. `name` is what a screen reader says,
/// since the number itself fills the place of a hint. Returns true once for a new value.
pub fn number_field(ui: &mut Ui, value: &mut u64, name: &str, width: f32) -> bool {
    let id = ui.next_auto_id().with("typed");
    let mut typed: String = ui
        .data_mut(|data| data.get_temp(id))
        .unwrap_or_else(|| value.to_string());
    let response = field(ui, &mut typed, name, width);
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
    let fill = if *on { t.main_bottom } else { t.line_strong };
    ui.painter().rect_filled(rect, CornerRadius::same(10), fill);
    let x = egui::lerp(rect.left() + radius..=rect.right() - radius, how_far);
    let knob = if *on { t.main_text } else { t.text };
    ui.painter().circle_filled(pos2(x, rect.center().y), radius - 3.0, knob);
    focus_ring(ui, rect, &response, t);
    let state = *on;
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Switch, true, state, label));
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

fn segment(ui: &mut Ui, t: &Tokens, label: &str, current: bool) -> Response {
    let font = TextStyle::Button.resolve(ui.style());
    let colour = if current { t.text } else { t.soft };
    let galley = layout(ui, label, font, colour);
    let size = vec2(galley.size().x + 24.0, tokens::CONTROL - 6.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let area = pressed_area(ui, &response, rect);
    if current {
        let look = Surface {
            top: t.float_top,
            bottom: t.float_bottom,
            line: t.line,
            glint: t.glint,
            shadow: None,
            radius: tokens::RADIUS_CONTROL,
        };
        surface(ui.painter(), area, &look, None);
    } else if response.hovered() {
        ui.painter().rect_filled(area, CornerRadius::same(tokens::RADIUS_CONTROL), t.hover);
    }
    paint_centred(ui, area, galley, colour);
    focus_ring(ui, rect, &response, t);
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Tab, true, current, label));
    response
}

/// Choices side by side in one tray, one of them current. Returns the one clicked.
pub fn segments<T: Copy + PartialEq>(
    ui: &mut Ui,
    t: &Tokens,
    items: &[(T, String)],
    current: Option<T>,
) -> Option<T> {
    let mut chosen = None;
    let tray = Frame::new()
        .fill(t.hover)
        .stroke(Stroke::new(1.0, t.line))
        .corner_radius(CornerRadius::same(tokens::RADIUS_BUTTON))
        .inner_margin(Margin::same(2));
    tray.show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for (value, label) in items {
                if segment(ui, t, label, current == Some(*value)).clicked() {
                    chosen = Some(*value);
                }
            }
        });
    });
    chosen
}

/// What is true, and the next step as a button. Returns true when it is pressed.
pub fn empty(ui: &mut Ui, t: &Tokens, heading: &str, body: &str, action: Option<&str>) -> bool {
    let mut pressed = false;
    card(ui, t, |ui| {
        ui.add_space(tokens::GAP_PAGE);
        ui.vertical_centered(|ui| {
            section(ui, heading);
            ui.add_space(2.0);
            meta(ui, body, t);
            if let Some(action) = action {
                ui.add_space(tokens::GAP);
                pressed = button(ui, t, action, Kind::Main).clicked();
            }
        });
        ui.add_space(tokens::GAP_PAGE);
    });
    pressed
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

/// Holds what comes and goes with the data. egui names a widget by how many came before it, so a
/// line that appears above a button renames that button, and a press on it is lost, with the typing
/// focus of a field. With this around it, what follows keeps its name.
pub fn slot<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    let inner = ui.scope(add);
    if inner.response.rect.height() <= 0.0 {
        ui.add_space(-ui.spacing().item_spacing.y);
    }
    inner.inner
}

/// Text cut to its column.
pub fn cell(ui: &mut Ui, text: impl Into<WidgetText>) {
    ui.add(Label::new(text).truncate());
}

/// Text at the right edge of its column, as figures stand in a ledger.
pub fn right_cell(ui: &mut Ui, text: impl Into<WidgetText>) {
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        ui.add(Label::new(text).truncate());
    });
}

pub fn header_cell(ui: &mut Ui, t: &Tokens, text: &str) {
    let text = RichText::new(text).size(tokens::META).color(t.muted);
    ui.add(Label::new(text).truncate().selectable(false));
}

pub fn divider(ui: &mut Ui, t: &Tokens) {
    let rect = ui.available_rect_before_wrap();
    fading_line(ui.painter(), rect.left(), rect.right(), rect.top(), t.line_strong);
    ui.add_space(1.0);
}

/// A row that can be chosen, with a wash on hover and a surface when current.
pub fn pick_row(ui: &mut Ui, t: &Tokens, height: f32, current: bool, name: &str, add: impl FnOnce(&mut Ui)) -> Response {
    let size = vec2(ui.available_width(), height);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    if current {
        let look = Surface {
            top: t.float_top,
            bottom: t.float_bottom,
            line: t.line,
            glint: t.glint,
            shadow: None,
            radius: tokens::RADIUS_BUTTON,
        };
        surface(ui.painter(), rect, &look, None);
    } else if response.hovered() {
        ui.painter().rect_filled(rect, CornerRadius::same(tokens::RADIUS_BUTTON), t.hover);
    }
    let inner = rect.shrink2(vec2(12.0, 0.0));
    let mut row = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(Layout::left_to_right(Align::Center)));
    add(&mut row);
    focus_ring(ui, rect, &response, t);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, name));
    response
}

/// What a [`slider`] did this frame.
pub struct Slid {
    pub response: Response,
    /// The hand let go, was clicked or a key moved it: the value to act on, once.
    pub settled: bool,
}

const SLIDER_TRACK: f32 = 6.0;
const SLIDER_KNOB: f32 = 9.0;

/// A track with a lit knob, in the look of the other controls. `value` follows the hand while it
/// drags, and [`Slid::settled`] says when to act on it, so a change that costs something waits for
/// the final value. `marks` are places on the track, as parts of 1, drawn as small notches.
pub fn slider(ui: &mut Ui, t: &Tokens, value: &mut f32, range: std::ops::RangeInclusive<f32>, name: &str, marks: &[f32]) -> Slid {
    let width = ui.available_width().max(80.0);
    let (rect, response) = ui.allocate_exact_size(vec2(width, tokens::CONTROL), Sense::click_and_drag());
    let (low, high) = (*range.start(), *range.end());
    let span = (high - low).max(f32::EPSILON);
    let track = Rect::from_center_size(rect.center(), vec2(width - SLIDER_KNOB * 2.0, SLIDER_TRACK));
    let mut settled = response.drag_stopped() || response.clicked();
    if let Some(pointer) = response.interact_pointer_pos().filter(|_| response.dragged() || response.clicked()) {
        let part = ((pointer.x - track.left()) / track.width()).clamp(0.0, 1.0);
        *value = low + part * span;
    }
    if response.has_focus() {
        let step = span / 100.0;
        let (left, right) = ui.input(|input| (input.key_pressed(egui::Key::ArrowLeft), input.key_pressed(egui::Key::ArrowRight)));
        if left || right {
            *value = (*value + if right { step } else { -step }).clamp(low, high);
            settled = true;
        }
    }
    let part = ((*value - low) / span).clamp(0.0, 1.0);
    let painter = ui.painter();
    let radius = CornerRadius::same((SLIDER_TRACK / 2.0) as u8);
    painter.rect_filled(track, radius, t.line_strong);
    let filled = Rect::from_min_max(track.min, pos2(track.left() + track.width() * part, track.bottom()));
    painter.rect_filled(filled, radius, t.info);
    for mark in marks {
        let x = track.left() + track.width() * mark.clamp(0.0, 1.0);
        let colour = if *mark <= part { t.card_top } else { t.muted };
        painter.circle_filled(pos2(x, track.center().y), 1.6, colour);
    }
    let knob = pos2(track.left() + track.width() * part, track.center().y);
    let lift = if response.hovered() || response.dragged() { 1.0 } else { 0.0 };
    painter.circle_filled(knob + vec2(0.0, 1.5), SLIDER_KNOB + lift, t.shadow.gamma_multiply(0.35));
    painter.circle_filled(knob, SLIDER_KNOB + lift, t.float_top);
    painter.circle_stroke(knob, SLIDER_KNOB + lift, Stroke::new(1.0, t.line_strong));
    painter.circle_filled(knob, 3.0, t.info);
    focus_ring(ui, rect, &response, t);
    let shown = f64::from(*value);
    response.widget_info(|| egui::WidgetInfo::slider(true, shown, name));
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    Slid { response, settled }
}
