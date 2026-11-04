//! The row of "+ kind" tiles that each add one participant, the one-click setups, "clear all",
//! and how many more this computer carries.
use super::lab_glyphs as glyphs;
use super::lab_tokens::{kind_key, number};
use crate::i18n::{fill, text, Voice};
use crate::local::lab_net::{Kind, Limit, Order, Room, Scene, MOST};
use crate::ui::tokens::{self, Tokens};
use crate::ui::widgets::{button_on, Kind as ButtonKind};
use egui::{vec2, Align2, CornerRadius, FontId, RichText, Sense, Stroke, StrokeKind, Ui};

const TILE_HEIGHT: f32 = 40.0;
const GLYPH: f32 = 18.0;

pub fn scene_key(scene: Scene) -> &'static str {
    match scene {
        Scene::Race => "net.scene_race",
        Scene::Pool => "net.scene_pool",
        Scene::Payment => "net.scene_payment",
        Scene::Attack => "lab2.kind_attack",
        Scene::Split => "lab2.kind_split",
        Scene::Spam => "net.scene_spam",
        Scene::Exploit => "net.scene_exploit",
    }
}

/// One tile: the kind's shape and its name after a plus. Its accessible name is the kind's name.
fn tile(ui: &mut Ui, t: &Tokens, voice: Voice, kind: Kind, enabled: bool) -> bool {
    let words = format!("+ {}", text(voice, kind_key(kind)));
    let galley = ui.painter().layout_no_wrap(words.clone(), FontId::proportional(tokens::TEXT), t.text);
    let width = galley.size().x + GLYPH + 30.0;
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, response) = ui.allocate_exact_size(vec2(width, TILE_HEIGHT), sense);
    let name = text(voice, kind_key(kind));
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, format!("+ {name}")));
    let painter = ui.painter();
    let hovered = enabled && response.hovered();
    let back = if hovered { t.float_top } else { t.card_top };
    painter.rect_filled(rect, CornerRadius::same(tokens::RADIUS_BUTTON), back);
    painter.rect_stroke(rect, CornerRadius::same(tokens::RADIUS_BUTTON), Stroke::new(1.0, if hovered { t.line_strong } else { t.line }), StrokeKind::Inside);
    let disc = rect.left_center() + vec2(8.0 + GLYPH / 2.0, 0.0);
    painter.circle_filled(disc, GLYPH / 2.0 + 3.0, glyphs::fill(t, kind));
    glyphs::paint(painter, kind, disc, GLYPH - 2.0, glyphs::ink(t, kind));
    let colour = if enabled { t.text } else { t.muted };
    painter.text(rect.left_center() + vec2(GLYPH + 18.0, 0.0), Align2::LEFT_CENTER, words, FontId::proportional(tokens::TEXT), colour);
    response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

/// The tiles, the setups and "clear all". Returns what was pressed.
/// `hashing` is what the miners were set to together and the person's speed limit, when one
/// is set.
pub fn draw(ui: &mut Ui, t: &Tokens, voice: Voice, room: &Room, running: usize, hashing: Option<(u64, u64)>) -> Option<Order> {
    let mut order = None;
    ui.label(RichText::new(text(voice, "net.add")).color(t.soft).size(tokens::META));
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
        for kind in Kind::ALL {
            if tile(ui, t, voice, kind, !room.full()) {
                order = Some(Order::Add(kind));
            }
        }
    });
    ui.add_space(tokens::GAP);
    ui.label(RichText::new(text(voice, "net.scenes")).color(t.soft).size(tokens::META));
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
        for scene in Scene::ALL {
            if button_on(ui, t, text(voice, scene_key(scene)), ButtonKind::Plain, true).clicked() {
                order = Some(Order::Scene(scene));
            }
        }
        if button_on(ui, t, text(voice, "net.clear"), ButtonKind::Quiet, running > 0).clicked() {
            order = Some(Order::Clear);
        }
    });
    ui.add_space(4.0);
    let (mut line, colour) = room_line(t, voice, room);
    // Past the limit every miner slows by the same part; below it, what is left.
    let rate = |value: u64| crate::ui::format::rate(voice, value);
    // After a sentence a space; after the expert's list of figures, one more of its dots.
    let join = if line.ends_with('.') { " " } else { " · " };
    match hashing {
        Some((asked, limit)) if asked > limit => line = format!("{line}{join}{}", fill(text(voice, "net.hash_scaled"), &[&rate(limit)])),
        Some((asked, limit)) => line = format!("{line}{join}{}", fill(text(voice, "net.hash_room"), &[&rate(limit - asked)])),
        None => {}
    }
    ui.label(RichText::new(line).color(colour).size(tokens::META));
    order
}

/// How many more fit, or the one plain reason no more do.
fn room_line(t: &Tokens, voice: Voice, room: &Room) -> (String, egui::Color32) {
    if room.full() {
        let line = match room.limit {
            Limit::Memory => text(voice, "net.room_memory").to_string(),
            Limit::Cores => text(voice, "net.room_cores").to_string(),
            Limit::Map => fill(text(voice, "net.room_map"), &[&number(MOST)]),
        };
        return (line, t.warn);
    }
    // Expert words also say the free memory and the cores.
    let values = [number(room.more), number(room.running), number(room.free_mib), number(room.cores)];
    let line = fill(text(voice, "net.room"), &values.iter().map(String::as_str).collect::<Vec<_>>());
    (line, t.muted)
}
