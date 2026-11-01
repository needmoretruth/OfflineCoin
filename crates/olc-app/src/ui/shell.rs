//! The window around a page: the header with the coin and what the node is doing, the side bar,
//! and the notice at the bottom.
use super::format;
use super::icons;
use super::logo::{Logo, LogoState};
use super::paint::{gradient, surface, Surface};
use super::tokens::{self, Tokens};
use super::widgets::{button, strong_text, Kind};
use crate::core::{Notice, Snapshot};
use crate::i18n::{self, Voice};
use crate::pages::Page;
use egui::{pos2, vec2, Align, Color32, CornerRadius, Frame, Label, Layout, Margin, Panel, Rect, RichText, Sense, Ui};

pub fn paint_back(ui: &Ui, t: &Tokens) {
    let rect = ui.max_rect();
    ui.painter().add(gradient(rect, 0.0, t.back_top, t.back_bottom));
}

fn side_row(ui: &mut Ui, t: &Tokens, icon: icons::Icon, text: &str, current: bool) -> bool {
    let size = vec2(ui.available_width(), tokens::SIDE_ROW);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let hover = response.hovered();
    if current || hover {
        let look = Surface {
            top: if current { t.card_top } else { t.hover },
            bottom: if current { t.card_bottom } else { t.hover },
            line: if current { t.line } else { Color32::TRANSPARENT },
            glint: if current { t.glint } else { Color32::TRANSPARENT },
            shadow: None,
            radius: tokens::RADIUS_CONTROL,
        };
        surface(ui.painter(), rect, &look, None);
    }
    let colour = if current || hover { t.text } else { t.muted };
    let icon_area = Rect::from_center_size(
        pos2(rect.left() + 20.0, rect.center().y),
        vec2(tokens::ICON, tokens::ICON),
    );
    icons::paint(ui.painter(), icon_area, icon, colour);
    let galley = ui.painter().layout_no_wrap(
        text.to_string(),
        egui::TextStyle::Body.resolve(ui.style()),
        colour,
    );
    let at = pos2(rect.left() + 40.0, rect.center().y - galley.size().y / 2.0);
    ui.painter().galley(at, galley, colour);
    if response.has_focus() {
        ui.painter().rect_stroke(
            rect.expand(1.0),
            CornerRadius::same(tokens::RADIUS_CONTROL),
            egui::Stroke::new(1.5, t.info),
            egui::StrokeKind::Outside,
        );
    }
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, text));
    response.clicked()
}

/// Returns the page clicked.
pub fn side_bar(
    ui: &mut Ui,
    t: &Tokens,
    voice: Voice,
    pages: &[Box<dyn Page>],
    current: usize,
) -> Option<usize> {
    let mut chosen = None;
    let frame = Frame::new().fill(t.side).inner_margin(Margin::same(10));
    Panel::left("side")
        .exact_size(tokens::SIDEBAR)
        .resizable(false)
        .frame(frame)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.add_space(6.0);
            let (main, apart): (Vec<_>, Vec<_>) =
                pages.iter().enumerate().partition(|(_, page)| !page.apart());
            for (index, page) in main {
                if side_row(ui, t, page.icon(), i18n::text(voice, page.title()), index == current) {
                    chosen = Some(index);
                }
            }
            let rest = ui.available_height() - apart.len() as f32 * (tokens::SIDE_ROW + 2.0) - 8.0;
            ui.add_space(rest.max(tokens::GAP));
            for (index, page) in apart {
                if side_row(ui, t, page.icon(), i18n::text(voice, page.title()), index == current) {
                    chosen = Some(index);
                }
            }
        });
    chosen
}

fn strip_item(ui: &mut Ui, t: &Tokens, name: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 5.0;
        let name = RichText::new(name).size(tokens::META).color(t.muted);
        ui.add(Label::new(name).selectable(false));
        ui.add(Label::new(strong_text(value).size(tokens::META)).selectable(false));
    });
    ui.add_space(10.0);
}

pub struct HeaderOut {
    pub language_clicked: bool,
}

/// The strip of figures waits until a coin has been chosen.
pub struct HeaderLook {
    pub still: bool,
    pub strip: bool,
}

pub fn header(
    ui: &mut Ui,
    t: &Tokens,
    voice: Voice,
    snap: &Snapshot,
    logo: &mut Logo,
    look: &HeaderLook,
) -> HeaderOut {
    let mut out = HeaderOut { language_clicked: false };
    let frame = Frame::new().fill(t.side).inner_margin(Margin::symmetric(14, 0));
    Panel::top("header")
        .exact_size(tokens::HEADER)
        .resizable(false)
        .frame(frame)
        .show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                let state = LogoState {
                    mining: snap.node.mining,
                    tip_changes: snap.node.tip_changes,
                    still: look.still,
                };
                logo.show(ui, tokens::LOGO, (t.logo, t.logo_floor), &state);
                let name = strong_text("OLC Core").size(tokens::SECTION + 1.0);
                ui.add(Label::new(name).selectable(false));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let other = i18n::text(voice, "language.other");
                    out.language_clicked = button(ui, t, other, Kind::Plain).clicked();
                    ui.add_space(tokens::GAP_WIDE);
                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                        ui.add_space(tokens::GAP_WIDE);
                        if look.strip {
                            strip(ui, t, voice, snap);
                        }
                    });
                });
            });
        });
    let rect = ui.max_rect();
    let y = rect.top() - 0.5;
    ui.painter().hline(rect.x_range(), y, egui::Stroke::new(1.0, t.line));
    out
}

fn strip(ui: &mut Ui, t: &Tokens, voice: Voice, snap: &Snapshot) {
    let node = &snap.node;
    let Some(coin) = &node.coin else {
        return;
    };
    let say = |key| i18n::text(voice, key);
    strip_item(ui, t, say("strip.coin"), &coin.name);
    strip_item(ui, t, say("strip.height"), &format::number(node.height.into()));
    strip_item(ui, t, say("strip.peers"), &node.peers.to_string());
    if node.mining {
        strip_item(ui, t, say("strip.hash"), &format::rate(node.hashrate));
    }
    strip_item(ui, t, say("strip.balance"), &format::coins(node.balance, &coin.ticker));
}

pub fn notice_bar(ui: &mut Ui, t: &Tokens, voice: Voice, notice: &Notice) {
    let colour = if notice.good { t.good } else { t.bad };
    let frame = Frame::new().fill(t.side).inner_margin(Margin::symmetric(tokens::PAGE_MARGIN, 8));
    Panel::bottom("notice").frame(frame).show(ui, |ui| {
        ui.horizontal(|ui| {
            super::widgets::dot(ui, colour);
            let text = i18n::text(voice, notice.key);
            let line = i18n::fill(text, &[notice.detail.as_str()]);
            ui.add(Label::new(line).wrap());
        });
    });
}
