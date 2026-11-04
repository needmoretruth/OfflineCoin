//! What floats over a page: a slip read before signing, a sheet that holds a small job, and the
//! notice. This is the third layer, and only it has a shadow.
use super::paint::{fading_line, ink, surface, Surface};
use super::tokens::{self, Tokens};
use super::widgets::{button, mono, section, strong_text, Kind};
use crate::core::Command;
use egui::epaint::Shadow;
use egui::{Area, Color32, Frame, Id, Label, Layout, Align, Margin, Modal, Order, RichText, Stroke, Ui};

const SHORT_VALUE: usize = 28;

fn floating_look(t: &Tokens, radius: u8) -> Surface {
    Surface {
        top: t.float_top,
        bottom: t.float_bottom,
        line: t.line,
        glint: t.glint,
        shadow: Some(Shadow { offset: [0, 12], blur: 32, spread: 0, color: t.shadow }),
        radius,
    }
}

/// A floating surface in the middle of the window, with the page dimmed behind it. Returns true
/// when the person dismissed it from outside or with Escape.
pub fn sheet<R>(ui: &Ui, t: &Tokens, id: &str, width: f32, add: impl FnOnce(&mut Ui) -> R) -> (R, bool) {
    let frame = Frame::new().inner_margin(Margin::same(24));
    let area = Modal::default_area(Id::unique(id)).default_width(width);
    let modal = Modal::new(Id::unique(id)).area(area).frame(Frame::new()).backdrop_color(t.scrim);
    let shown = modal.show(ui.ctx(), |ui| {
        ui.set_width(width);
        let slot = ui.painter().add(egui::Shape::Noop);
        let inner = frame.show(ui, |ui| {
            ui.set_width(width - 48.0);
            add(ui)
        });
        surface(ui.painter(), inner.response.rect, &floating_look(t, tokens::RADIUS_SHEET), Some(slot));
        inner.inner
    });
    let closed = shown.should_close();
    (shown.inner, closed)
}

/// What a person reads before signing. It holds the order it will send.
pub struct Slip {
    pub what: String,
    /// The largest thing on the slip, when money moves.
    pub amount: Option<String>,
    pub rows: Vec<(String, String)>,
    /// One plain sentence on what cannot be undone.
    pub warning: Option<String>,
    pub sign: String,
    pub cancel: String,
    pub danger: bool,
    pub order: Vec<Command>,
}

pub enum SlipAnswer {
    Waiting,
    Signed,
    Cancelled,
}

fn slip_amount(ui: &mut Ui, t: &Tokens, amount: &str) {
    let font = egui::FontId::new(tokens::SLIP_AMOUNT, super::fonts::strong());
    let galley = ui.painter().layout_no_wrap(amount.to_string(), font, t.text);
    let size = galley.size();
    let lit = ink(&galley, t.ink_top, t.ink_bottom);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::hover());
    ui.painter().galley(rect.min, lit, t.text);
    let label = amount.to_string();
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, label.as_str()));
}

/// A short value ends its line, as on a receipt. A long one, like an address, gets its own line.
fn slip_row(ui: &mut Ui, t: &Tokens, name: &str, value: &str) {
    let label = RichText::new(name).color(t.muted);
    if value.chars().count() > SHORT_VALUE {
        ui.add(Label::new(label).selectable(false));
        ui.add(Label::new(mono(value)).wrap());
    } else {
        ui.horizontal(|ui| {
            ui.add(Label::new(label).selectable(false));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| ui.add(Label::new(value)));
        });
    }
    let y = ui.cursor().top() + 1.0;
    fading_line(ui.painter(), ui.min_rect().left(), ui.min_rect().right(), y, t.line);
    ui.add_space(6.0);
}

/// Draws the slip, top to bottom: what, how much, the rest, then the signature.
pub fn show_slip(ui: &Ui, t: &Tokens, slip: &Slip) -> SlipAnswer {
    let mut answer = SlipAnswer::Waiting;
    let (_, closed) = sheet(ui, t, "slip", tokens::SLIP_WIDTH, |ui| {
        if slip.danger {
            let rect = ui.max_rect();
            let y = rect.top() - 23.0;
            let line = Stroke::new(2.0, t.bad);
            ui.painter().hline(rect.x_range(), y, line);
        }
        section(ui, &slip.what);
        if let Some(amount) = &slip.amount {
            ui.add_space(tokens::GAP);
            slip_amount(ui, t, amount);
        }
        ui.add_space(tokens::GAP_WIDE);
        for (name, value) in &slip.rows {
            slip_row(ui, t, name, value);
        }
        if let Some(warning) = &slip.warning {
            ui.add_space(tokens::GAP);
            ui.add(Label::new(RichText::new(warning).color(if slip.danger { t.bad } else { t.soft })).wrap());
        }
        ui.add_space(tokens::GAP_PAGE - tokens::GAP);
        ui.horizontal(|ui| {
            let kind = if slip.danger { Kind::Danger } else { Kind::Main };
            if button(ui, t, &slip.sign, kind).clicked() {
                answer = SlipAnswer::Signed;
            }
            if button(ui, t, &slip.cancel, Kind::Plain).clicked() {
                answer = SlipAnswer::Cancelled;
            }
        });
    });
    if closed {
        return SlipAnswer::Cancelled;
    }
    answer
}

/// The heading of a sheet, with its way out.
pub fn sheet_head(ui: &mut Ui, title: &str) {
    ui.add(Label::new(strong_text(title).size(tokens::TITLE)).selectable(false));
    ui.add_space(tokens::GAP);
}

/// What a page asks of the notice: no notice numbered after `.0` up to `.1` floats, since the
/// page itself says it; and this frame, none lower than `floor`, so it covers no field.
#[derive(Clone, Copy, Default)]
pub struct NoticeRoom {
    pub quiet: (u64, u64),
    floor: Option<(u64, f32)>,
}

fn room_id() -> Id {
    Id::unique("notice-room")
}

/// The room a page asked for, kept between frames.
pub fn notice_room(ctx: &egui::Context) -> NoticeRoom {
    ctx.data(|data| data.get_temp(room_id())).unwrap_or_default()
}

/// Keeps the notices numbered after `after` up to `through` from floating.
pub fn quiet_notices(ctx: &egui::Context, after: u64, through: u64) {
    let room = NoticeRoom { quiet: (after, through), ..notice_room(ctx) };
    ctx.data_mut(|data| data.insert_temp(room_id(), room));
}

/// Keeps this frame's notice above `y`.
pub fn notice_above(ctx: &egui::Context, y: f32) {
    let room = NoticeRoom { floor: Some((ctx.cumulative_pass_nr(), y)), ..notice_room(ctx) };
    ctx.data_mut(|data| data.insert_temp(room_id(), room));
}

/// The result of what a person did, over the bottom right of the window.
pub fn notice(ui: &Ui, t: &Tokens, colour: Color32, line: &str) {
    let screen = ui.ctx().content_rect();
    let width = (screen.width() * 0.5).clamp(280.0, 520.0);
    let room = notice_room(ui.ctx());
    let pass = ui.ctx().cumulative_pass_nr();
    let floor = room.floor.filter(|(asked, _)| *asked == pass).map_or(screen.bottom() - tokens::GAP_PAGE, |(_, y)| y - tokens::GAP);
    let area = Area::new(Id::unique("notice"))
        .order(Order::Tooltip)
        .interactable(false)
        .pivot(egui::Align2::RIGHT_BOTTOM)
        .fixed_pos(egui::pos2(screen.right() - tokens::GAP_PAGE, floor));
    area.show(ui.ctx(), |ui| {
        let slot = ui.painter().add(egui::Shape::Noop);
        let inner = Frame::new().inner_margin(Margin::symmetric(14, 10)).show(ui, |ui| {
            ui.set_max_width(width);
            ui.horizontal_top(|ui| {
                ui.add_space(0.0);
                super::widgets::dot(ui, colour);
                ui.add(Label::new(line).wrap());
            });
        });
        surface(ui.painter(), inner.response.rect, &floating_look(t, tokens::RADIUS_CARD), Some(slot));
    });
}
