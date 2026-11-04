//! The Lab's map: each participant a token with its shape and a short name, lines for the
//! connections each one made, and a pool as a ring holding its miners. What happens moves on it: a
//! found block pulses on its finder and travels to the others, a payment travels as a blue dot,
//! shares and payouts go between a pool and its miners, and a refusal, a payment settled or undone,
//! and blocks replaced flash on the node with one short line. A miner dragged into a pool joins it.
use super::lab_glyphs as glyphs;
use super::lab_tokens::{name, status_line};
use crate::i18n::{fill, text, Voice};
use crate::local::lab_net::{Kind, Member, NetState, What};
use crate::ui::tokens::{self, Tokens};
use egui::{vec2, Align2, Color32, CornerRadius, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2};
use std::collections::BTreeMap;
use std::f32::consts::TAU;

pub const TOKEN: f32 = 20.0;
/// Seconds each kind of moment stays on the map.
const BLOCK_SECONDS: f64 = 0.9;
const PAY_SECONDS: f64 = 1.4;
const SHARE_SECONDS: f64 = 0.7;
const FLASH_SECONDS: f64 = 3.0;
/// Found blocks drawn at once: making many blocks fast would fill the map with dots.
const MOST_BLOCK_FLIGHTS: usize = 3;

struct Flight {
    what: What,
    born: f64,
}

/// What the map remembers between frames.
#[derive(Default)]
pub struct MapView {
    pub selected: Option<u32>,
    seen: u64,
    flights: Vec<Flight>,
    dragging: Option<u32>,
}

pub enum MapAct {
    Select(Option<u32>),
    Join(u32, Option<u32>),
}

/// Where each token sits, and how wide each pool's ring is.
struct Places {
    at: BTreeMap<u32, Pos2>,
    rings: BTreeMap<u32, f32>,
}

fn in_pool(member: &Member, members: &[Member]) -> bool {
    member.pool.is_some_and(|pool| members.iter().any(|other| other.id == pool))
}

/// Room inside a pool for one miner: its token, its name and its line.
const CELL: Vec2 = vec2(88.0, 68.0);
/// Below a ring or a token: its name and its line.
const TEXT_BELOW: f32 = 36.0;

/// Columns and rows of the miners inside a pool.
fn grid(inside: usize) -> (usize, usize) {
    let columns = match inside {
        0 | 1 => 1,
        2..=4 => 2,
        5 | 6 => 3,
        _ => 4,
    };
    (columns, inside.div_ceil(columns).max(1))
}

/// A pool's ring holds its miners' grid with a little room around it.
fn ring_radius(inside: usize) -> f32 {
    if inside == 0 {
        return TOKEN + 26.0;
    }
    let (columns, rows) = grid(inside);
    let half = vec2(columns as f32 * CELL.x, rows as f32 * CELL.y) / 2.0;
    half.length() + 6.0
}

/// The map's height for these members: the given height, or more for a large pool.
pub fn height_for(members: &[Member], least: f32) -> f32 {
    let widest = members
        .iter()
        .filter(|member| member.kind == Kind::Pool)
        .map(|pool| ring_radius(members.iter().filter(|member| member.pool == Some(pool.id)).count()))
        .fold(TOKEN, f32::max);
    least.max(widest * 2.0 + TEXT_BELOW + 28.0)
}

/// Tokens on an ellipse in the order they came, pools with their miners inside.
fn places(members: &[Member], rect: Rect) -> Places {
    let outer: Vec<&Member> = members.iter().filter(|member| !in_pool(member, members)).collect();
    let mut rings = BTreeMap::new();
    for pool in members.iter().filter(|member| member.kind == Kind::Pool) {
        let inside = members.iter().filter(|member| member.pool == Some(pool.id)).count();
        rings.insert(pool.id, ring_radius(inside));
    }
    let widest = rings.values().copied().fold(TOKEN, f32::max);
    let (rx, ry) = ((rect.width() / 2.0 - widest - 40.0).max(20.0), (rect.height() / 2.0 - widest - TEXT_BELOW).max(10.0));
    let mut at = BTreeMap::new();
    let count = outer.len();
    // Two side by side read better than one above the other; more go round from the top.
    let first = if count == 2 { TAU / 2.0 } else { -TAU / 4.0 };
    for (index, member) in outer.iter().enumerate() {
        let place = match count {
            1 => rect.center(),
            _ => {
                let angle = first + index as f32 * TAU / count as f32;
                rect.center() + vec2(angle.cos() * rx, angle.sin() * ry)
            }
        };
        // A token or a ring stays inside the map with its name under it.
        let reach = rings.get(&member.id).copied().unwrap_or(TOKEN);
        let x = place.x.clamp(rect.left() + reach + 8.0, (rect.right() - reach - 8.0).max(rect.left() + reach + 8.0));
        let y = place.y.clamp(rect.top() + reach + 8.0, (rect.bottom() - reach - TEXT_BELOW).max(rect.top() + reach + 8.0));
        at.insert(member.id, Pos2::new(x, y));
    }
    for (pool, _) in rings.clone() {
        let Some(centre) = at.get(&pool).copied() else {
            continue;
        };
        let inside: Vec<u32> = members.iter().filter(|member| member.pool == Some(pool)).map(|member| member.id).collect();
        let (columns, rows) = grid(inside.len());
        let corner = centre - vec2(columns as f32 * CELL.x, rows as f32 * CELL.y) / 2.0;
        for (index, id) in inside.iter().enumerate() {
            let (column, row) = (index % columns, index / columns);
            // The last row stands centred when it is not full.
            let in_row = if row + 1 == rows { inside.len() - row * columns } else { columns };
            let shift = (columns - in_row) as f32 * CELL.x / 2.0;
            let place = corner + vec2(shift + (column as f32 + 0.5) * CELL.x, row as f32 * CELL.y + TOKEN + 4.0);
            at.insert(*id, place);
        }
    }
    Places { at, rings }
}

fn along(from: Pos2, to: Pos2, part: f32) -> Pos2 {
    from + (to - from) * part.clamp(0.0, 1.0)
}

/// Draws the map in `height` and says what the hand did.
pub fn draw(ui: &mut Ui, t: &Tokens, voice: Voice, view: &mut MapView, state: &NetState, height: f32) -> Option<MapAct> {
    let width = ui.available_width();
    let height = height_for(&state.members, height);
    let (rect, background) = ui.allocate_exact_size(vec2(width, height), Sense::click());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, CornerRadius::same(tokens::RADIUS_CARD), t.back_floor.gamma_multiply(0.6));
    painter.rect_stroke(rect, CornerRadius::same(tokens::RADIUS_CARD), Stroke::new(1.0, t.line), StrokeKind::Inside);
    let now = ui.input(|input| input.time);
    take_events(view, state, now);
    if state.members.is_empty() {
        painter.text(rect.center(), Align2::CENTER_CENTER, text(voice, "net.empty"), FontId::proportional(tokens::TEXT), t.muted);
        return background.clicked().then_some(MapAct::Select(None));
    }
    let places = places(&state.members, rect);
    let spot = |id: u32| places.at.get(&id).copied();
    // Lines first, under everything: who dialed whom.
    for member in state.members.iter().filter(|member| !in_pool(member, &state.members)) {
        for link in &member.links {
            let linked = state.members.iter().find(|other| other.id == *link);
            if let (Some(from), Some(to), Some(other)) = (spot(member.id), spot(*link), linked) {
                if !in_pool(other, &state.members) {
                    painter.line_segment([from, to], Stroke::new(1.0, t.line_strong));
                }
            }
        }
    }
    let pointer = ui.input(|input| input.pointer.hover_pos());
    let over_pool = view.dragging.and(pointer).and_then(|pointer| {
        places.rings.iter().find(|(pool, radius)| spot(**pool).is_some_and(|centre| centre.distance(pointer) <= **radius)).map(|(pool, _)| *pool)
    });
    for (pool, radius) in &places.rings {
        let Some(centre) = spot(*pool) else {
            continue;
        };
        let lit = over_pool == Some(*pool);
        let colour = if lit { t.info } else { glyphs::fill(t, Kind::Pool) };
        painter.circle_filled(centre, *radius, colour.gamma_multiply(0.12));
        painter.circle_stroke(centre, *radius, Stroke::new(if lit { 3.0 } else { 2.0 }, colour));
    }
    let mut act = None;
    // Pools first: what is registered later takes the pointer, so a miner inside a ring keeps its
    // own clicks and drags.
    let pools = state.members.iter().filter(|member| member.kind == Kind::Pool);
    let others = state.members.iter().filter(|member| member.kind != Kind::Pool);
    for member in pools.chain(others) {
        let Some(centre) = spot(member.id) else {
            continue;
        };
        let pool_radius = places.rings.get(&member.id).copied();
        let reach = pool_radius.unwrap_or(TOKEN);
        let hit = Rect::from_center_size(centre, vec2(reach * 2.0, reach * 2.0));
        // Named with its line, so a screen reader says what it does as the map shows it.
        let (line, _) = status_line(t, voice, member);
        let label = if line.is_empty() { name(voice, member) } else { format!("{}, {line}", name(voice, member)) };
        let response = ui.interact(hit, ui.scope_id().with(("lab-token", member.id)), Sense::click_and_drag());
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
        if response.drag_started() && member.kind == Kind::Miner {
            view.dragging = Some(member.id);
        }
        if response.drag_stopped() && view.dragging == Some(member.id) {
            view.dragging = None;
            if over_pool != member.pool {
                act = Some(MapAct::Join(member.id, over_pool));
            }
        } else if response.clicked() {
            act = Some(MapAct::Select(if view.selected == Some(member.id) { None } else { Some(member.id) }));
        }
        let ghost = (view.dragging == Some(member.id)).then_some(pointer).flatten();
        let drawn_at = ghost.unwrap_or(centre);
        let selected = view.selected == Some(member.id);
        match pool_radius {
            // The ring is the pool; its shape sits on the ring's top.
            Some(radius) => {
                let badge = centre - vec2(0.0, radius);
                painter.circle_filled(badge, 13.0, glyphs::fill(t, Kind::Pool));
                glyphs::paint(&painter, Kind::Pool, badge, 16.0, glyphs::ink(t, Kind::Pool));
                if selected {
                    painter.circle_stroke(centre, radius + 4.0, Stroke::new(2.5, t.info));
                }
                text_under(&painter, t, voice, member, centre + vec2(0.0, radius + 4.0));
            }
            None => {
                token(&painter, t, member, drawn_at, selected || response.hovered());
                if selected {
                    painter.circle_stroke(drawn_at, TOKEN + 5.0, Stroke::new(2.5, t.info));
                }
                text_under(&painter, t, voice, member, drawn_at + vec2(0.0, TOKEN + 4.0));
            }
        }
    }
    flights(&painter, t, voice, view, &places, now);
    if view.dragging.is_some() || !view.flights.is_empty() {
        ui.ctx().request_repaint();
    }
    if act.is_none() && background.clicked() {
        act = Some(MapAct::Select(None));
    }
    act
}

fn token(painter: &egui::Painter, t: &Tokens, member: &Member, centre: Pos2, lifted: bool) {
    let mut colour = glyphs::fill(t, member.kind);
    if member.paused {
        colour = colour.gamma_multiply(0.45);
    }
    if lifted {
        painter.circle_filled(centre + vec2(0.0, 2.0), TOKEN + 1.0, t.shadow.gamma_multiply(0.5));
    }
    painter.circle_filled(centre, TOKEN, colour);
    let ink = glyphs::ink(t, member.kind);
    glyphs::paint(painter, member.kind, centre, 22.0, if member.paused { ink.gamma_multiply(0.5) } else { ink });
    if member.paused {
        let badge = centre + vec2(TOKEN * 0.72, TOKEN * 0.72);
        painter.circle_filled(badge, 8.0, t.card_top);
        glyphs::paused(painter, badge, 12.0, t.soft);
    }
    if member.ended.is_some() {
        painter.circle_stroke(centre, TOKEN + 2.0, Stroke::new(2.0, t.bad));
    }
}

fn text_under(painter: &egui::Painter, t: &Tokens, voice: Voice, member: &Member, top: Pos2) {
    painter.text(top, Align2::CENTER_TOP, name(voice, member), FontId::proportional(12.5), t.text);
    let (line, colour) = status_line(t, voice, member);
    if !line.is_empty() {
        painter.text(top + vec2(0.0, 16.0), Align2::CENTER_TOP, line, FontId::proportional(11.5), colour);
    }
}

/// New events since the last frame become moments on the map.
fn take_events(view: &mut MapView, state: &NetState, now: f64) {
    let newest = state.events.last().map_or(0, |event| event.seq);
    if newest < view.seen {
        // A new network: its events start again from one.
        view.seen = 0;
        view.flights.clear();
    }
    let fresh: Vec<&crate::local::lab_net::Event> = state.events.iter().filter(|event| event.seq > view.seen).collect();
    let blocks = fresh.iter().filter(|event| matches!(event.what, What::Block { .. })).count();
    for (index, event) in fresh.iter().filter(|event| matches!(event.what, What::Block { .. })).enumerate() {
        if index + MOST_BLOCK_FLIGHTS >= blocks {
            view.flights.push(Flight { what: event.what.clone(), born: now });
        }
    }
    for event in fresh.iter().filter(|event| !matches!(event.what, What::Block { .. })) {
        view.flights.push(Flight { what: event.what.clone(), born: now });
    }
    view.seen = view.seen.max(newest);
    view.flights.retain(|flight| now - flight.born < life(&flight.what));
}

fn life(what: &What) -> f64 {
    match what {
        What::Block { .. } => BLOCK_SECONDS,
        What::Paid { .. } | What::Payout { .. } => PAY_SECONDS,
        What::Shares { .. } => SHARE_SECONDS,
        _ => FLASH_SECONDS,
    }
}

fn flights(painter: &egui::Painter, t: &Tokens, voice: Voice, view: &MapView, places: &Places, now: f64) {
    let spot = |id: u32| places.at.get(&id).copied();
    let flash = |at: Pos2, colour: Color32, words: &str, part: f32| {
        let fade = (1.0 - part).clamp(0.0, 1.0);
        painter.circle_stroke(at, TOKEN + 6.0 + part * 8.0, Stroke::new(3.0, colour.gamma_multiply(fade)));
        let top = at - vec2(0.0, TOKEN + 12.0);
        let galley = painter.layout_no_wrap(words.to_string(), FontId::proportional(12.0), colour);
        let back = Rect::from_center_size(top - vec2(0.0, galley.size().y / 2.0), galley.size() + vec2(10.0, 4.0));
        painter.rect_filled(back, CornerRadius::same(4), t.card_top.gamma_multiply(0.92));
        painter.galley(back.min + vec2(5.0, 2.0), galley, colour);
    };
    for flight in &view.flights {
        let part = ((now - flight.born) / life(&flight.what)) as f32;
        match &flight.what {
            What::Block { finder: Some(finder), .. } => {
                let Some(from) = spot(*finder) else {
                    continue;
                };
                let colour = if places.rings.contains_key(finder) { t.soft } else { t.text };
                painter.circle_stroke(from, TOKEN + part * 24.0, Stroke::new(2.5, colour.gamma_multiply(1.0 - part)));
                for to in places.at.iter().filter(|(id, _)| *id != finder).map(|(_, at)| *at) {
                    let dot = along(from, to, part * 1.15 - 0.1);
                    painter.rect_filled(Rect::from_center_size(dot, vec2(6.0, 6.0)), CornerRadius::same(1), colour.gamma_multiply(0.9));
                }
            }
            What::Paid { from, to: Some(to), .. } => {
                if let (Some(from), Some(to)) = (spot(*from), spot(*to)) {
                    painter.circle_filled(along(from, to, part), 6.0, t.info);
                }
            }
            What::Shares { from, to, .. } => {
                if let (Some(from), Some(to)) = (spot(*from), spot(*to)) {
                    painter.circle_filled(along(from, to, part), 3.0, t.soft);
                }
            }
            What::Payout { from, to, .. } => {
                if let (Some(from), Some(to)) = (spot(*from), spot(*to)) {
                    painter.circle_filled(along(from, to, part), 5.0, t.good);
                }
            }
            What::Replaced { at, blocks } => {
                if let Some(at) = spot(*at) {
                    flash(at, t.warn, &fill(text(voice, "net.flash_replaced"), &[&blocks.to_string()]), part);
                }
            }
            What::Refused { by, rule, trick } => {
                if let Some(at) = spot(*by) {
                    // Plain words name the rule; the trade's own name for it is what the node said.
                    let named = if voice.mode == crate::settings::Mode::Expert && !rule.is_empty() { rule.as_str() } else { text(voice, trick.rule_key()) };
                    flash(at, t.bad, named, part);
                    glyphs::cross(painter, at + vec2(TOKEN * 0.8, -TOKEN * 0.8), 14.0, t.bad);
                }
            }
            What::Settled { at, .. } => {
                if let Some(at) = spot(*at) {
                    flash(at, t.good, text(voice, "net.flash_settled"), part);
                    glyphs::check(painter, at + vec2(TOKEN * 0.8, -TOKEN * 0.8), 14.0, t.good);
                }
            }
            What::Undone { at, .. } => {
                if let Some(at) = spot(*at) {
                    flash(at, t.bad, text(voice, "net.flash_undone"), part);
                    glyphs::cross(painter, at + vec2(TOKEN * 0.8, -TOKEN * 0.8), 14.0, t.bad);
                }
            }
            _ => {}
        }
    }
}
