//! The window around a page: the light on the background and the side rail.
use super::icons;
use super::logo::{Logo, LogoState};
use super::paint::{lit_back, surface, Surface};
use super::tokens::{self, Tokens};
use super::widgets::{dot, strong_text};
use crate::core::Snapshot;
use crate::i18n::{self, Voice};
use crate::pages::Page;
use egui::{pos2, vec2, CornerRadius, Frame, Label, Margin, Panel, Rect, RichText, Sense, Ui};

pub fn paint_back(ui: &Ui, t: &Tokens) {
    ui.painter().add(lit_back(ui.max_rect(), t));
}

fn rail_row(ui: &mut Ui, t: &Tokens, icon: icons::Icon, text: &str, current: bool) -> bool {
    let size = vec2(ui.available_width(), tokens::RAIL_ROW);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let hover = response.hovered();
    if current {
        let look = Surface {
            top: t.card_top,
            bottom: t.card_bottom,
            line: t.line,
            glint: t.glint,
            shadow: None,
            radius: tokens::RADIUS_BUTTON,
        };
        surface(ui.painter(), rect, &look, None);
    } else if hover {
        ui.painter().rect_filled(rect, CornerRadius::same(tokens::RADIUS_BUTTON), t.hover);
    }
    let colour = if current || hover { t.text } else { t.soft };
    let icon_area = Rect::from_center_size(
        pos2(rect.left() + 21.0, rect.center().y),
        vec2(tokens::ICON, tokens::ICON),
    );
    icons::paint(ui.painter(), icon_area, icon, colour);
    let font = if current {
        egui::FontId::new(tokens::TEXT, super::fonts::medium())
    } else {
        egui::TextStyle::Body.resolve(ui.style())
    };
    let galley = ui.painter().layout_no_wrap(text.to_string(), font, colour);
    let at = pos2(rect.left() + 42.0, rect.center().y - galley.size().y / 2.0);
    ui.painter().galley(at, galley, colour);
    if response.has_focus() {
        ui.painter().rect_stroke(
            rect.expand(1.0),
            CornerRadius::same(tokens::RADIUS_BUTTON),
            egui::Stroke::new(1.5, t.info),
            egui::StrokeKind::Outside,
        );
    }
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, text));
    response.clicked()
}

/// The coin and the name of the app.
pub fn brand(ui: &mut Ui, t: &Tokens, snap: &Snapshot, logo: &mut Logo, still: bool) {
    ui.horizontal(|ui| {
        ui.add_space(6.0);
        let state = LogoState {
            mining: snap.node.mining,
            tip_changes: snap.node.tip_changes,
            still,
        };
        logo.show(ui, tokens::LOGO, (t.logo, t.logo_floor), &state);
        let name = strong_text("OLC Core").size(tokens::SECTION);
        ui.add(Label::new(name).selectable(false));
    });
}

/// How the node stands, in a word, at the foot of the rail.
fn state_line(ui: &mut Ui, t: &Tokens, voice: Voice, snap: &Snapshot) {
    let node = &snap.node;
    let (colour, key) = match () {
        () if node.loading.is_some() => (t.warn, "state.loading"),
        () if node.syncing_to.is_some() => (t.warn, "state.catching_up"),
        () if node.mining => (t.good, "state.mining"),
        () if node.peers > 0 => (t.info, "state.connected"),
        () => (t.muted, "state.alone"),
    };
    ui.horizontal(|ui| {
        ui.add_space(14.0);
        dot(ui, colour);
        let text = RichText::new(i18n::text(voice, key)).size(tokens::META).color(t.muted);
        ui.add(Label::new(text).selectable(false));
    });
}

/// Returns the page clicked.
pub fn rail(
    ui: &mut Ui,
    t: &Tokens,
    voice: Voice,
    snap: &Snapshot,
    pages: &[(usize, &dyn Page)],
    current: usize,
    (logo, still): (&mut Logo, bool),
) -> Option<usize> {
    let mut chosen = None;
    let window = ui.ctx().content_rect().width();
    let width = if window < tokens::NARROW_WINDOW { tokens::RAIL_NARROW } else { tokens::RAIL };
    Panel::left("rail")
        .exact_size(width)
        .resizable(false)
        .show_separator_line(false)
        .frame(Frame::new().inner_margin(Margin::symmetric(10, 14)))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            brand(ui, t, snap, logo, still);
            ui.add_space(tokens::GAP_WIDE);
            let (main, apart): (Vec<_>, Vec<_>) = pages.iter().copied().partition(|(_, page)| !page.apart());
            for (index, page) in main {
                if rail_row(ui, t, page.icon(), i18n::text(voice, page.title()), index == current) {
                    chosen = Some(index);
                }
            }
            let foot = 24.0 + tokens::GAP;
            let rest = ui.available_height() - apart.len() as f32 * (tokens::RAIL_ROW + 2.0) - foot;
            ui.add_space(rest.max(tokens::GAP));
            for (index, page) in apart {
                if rail_row(ui, t, page.icon(), i18n::text(voice, page.title()), index == current) {
                    chosen = Some(index);
                }
            }
            ui.add_space(tokens::GAP);
            state_line(ui, t, voice, snap);
        });
    let line = ui.max_rect();
    ui.painter().vline(line.left() - 0.5, line.y_range(), egui::Stroke::new(1.0, t.line));
    chosen
}
