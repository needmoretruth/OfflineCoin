//! The Lab's map: each participant a token with its shape, its name and its line in a cell of its
//! own, lines for the connections each one made, a pool as a ring holding its miners, and the pile
//! of payments waiting for a block. What happens moves on it: a found block pulses on its finder
//! and travels to the others, a payment travels as a blue dot, shares and payouts go between a pool
//! and its miners, and a refusal, a payment settled or undone and blocks replaced leave a short
//! mark on the node; the sentence for each is under the map. A miner dragged into a pool joins it.
use super::lab_glyphs as glyphs;
use super::lab_places::{places, Places, BADGE, TOKEN, WORDS_GAP, WORDS_WIDTH};
use super::lab_tokens::{name, status_line};
use crate::i18n::{text, Voice};
use crate::local::lab_net::{Kind, Member, NetState, What};
use crate::ui::tokens::{self, Tokens};
use egui::text::{LayoutJob, TextWrapping};
use egui::{vec2, Align2, Color32, CornerRadius, FontId, Galley, Painter, Pos2, Rect, Sense, Stroke, StrokeKind, Ui};
use std::sync::Arc;

/// Seconds each kind of moment stays on the map.
const BLOCK_SECONDS: f64 = 0.9;
const PAY_SECONDS: f64 = 1.4;
const SHARE_SECONDS: f64 = 0.7;
const MARK_SECONDS: f64 = 3.0;
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

fn in_pool(member: &Member, members: &[Member]) -> bool {
    member.pool.is_some_and(|pool| members.iter().any(|other| other.id == pool))
}

fn along(from: Pos2, to: Pos2, part: f32) -> Pos2 {
    from + (to - from) * part.clamp(0.0, 1.0)
}

/// A line of words cut to the width of a cell, with an ellipsis when it does not fit.
fn one_line(painter: &Painter, words: String, size: f32, colour: Color32) -> Arc<Galley> {
    let mut job = LayoutJob::simple_singleline(words, FontId::proportional(size), colour);
    job.wrap = TextWrapping { max_width: WORDS_WIDTH, max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
    painter.layout_job(job)
}

/// The name and the line under a token, laid out.
struct Words {
    name: Arc<Galley>,
    line: Option<Arc<Galley>>,
}

impl Words {
    fn of(painter: &Painter, t: &Tokens, voice: Voice, member: &Member) -> Self {
        let (line, colour) = status_line(t, voice, member);
        let line = (!line.is_empty()).then(|| one_line(painter, line, 11.5, colour));
        Self { name: one_line(painter, name(voice, member), 12.5, t.text), line }
    }

    /// Where they reach with their top middle at `top`.
    fn rect(&self, top: Pos2) -> Rect {
        let wide = self.line.as_ref().map_or(0.0, |line| line.size().x).max(self.name.size().x);
        let high = 16.0 + self.line.as_ref().map_or(0.0, |line| line.size().y);
        Rect::from_min_size(top - vec2(wide / 2.0, 0.0), vec2(wide, high))
    }

    fn paint(self, painter: &Painter, t: &Tokens, top: Pos2) {
        painter.galley(top - vec2(self.name.size().x / 2.0, 0.0), self.name, t.text);
        if let Some(line) = self.line {
            painter.galley(top + vec2(-line.size().x / 2.0, 16.0), line, t.text);
        }
    }
}

/// Draws the map, at least `least` high and taller as it fills, and says what the hand did.
pub fn draw(ui: &mut Ui, t: &Tokens, voice: Voice, view: &mut MapView, state: &NetState, least: f32) -> Option<MapAct> {
    let width = ui.available_width();
    let pile = state.members.iter().any(|member| matches!(member.kind, Kind::Sender | Kind::Spammer));
    let laid = places(&state.members, width, pile);
    let height = laid.height.max(least);
    let (rect, background) = ui.allocate_exact_size(vec2(width, height), Sense::click());
    background.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, text(voice, "net.map")));
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, CornerRadius::same(tokens::RADIUS_CARD), t.back_floor.gamma_multiply(0.6));
    painter.rect_stroke(rect, CornerRadius::same(tokens::RADIUS_CARD), Stroke::new(1.0, t.line), StrokeKind::Inside);
    let now = ui.input(|input| input.time);
    take_events(view, state, now);
    if state.members.is_empty() {
        painter.text(rect.center(), Align2::CENTER_CENTER, text(voice, "net.empty"), FontId::proportional(tokens::TEXT), t.muted);
        return background.clicked().then_some(MapAct::Select(None));
    }
    let below = (height - laid.height) / 2.0;
    let places = laid.moved(rect.min.to_vec2() + vec2(0.0, below));
    let spot = |id: u32| places.at.get(&id).copied();
    // Lines first, under everything: who dialed whom.
    for member in state.members.iter().filter(|member| !in_pool(member, &state.members)) {
        for link in &member.links {
            let linked = state.members.iter().find(|other| other.id == *link);
            if let (Some(from), Some(to), Some(other)) = (spot(member.id), spot(*link), linked) {
                if !in_pool(other, &state.members) {
                    painter.line_segment([from, to], Stroke::new(1.0, t.line));
                }
            }
        }
    }
    let pointer = ui.input(|input| input.pointer.hover_pos());
    let over_pool = view.dragging.and(pointer).and_then(|pointer| places.rings.iter().find(|(_, ring)| ring.contains(pointer)).map(|(pool, _)| *pool));
    for (pool, ring) in &places.rings {
        let lit = over_pool == Some(*pool);
        let colour = if lit { t.info } else { glyphs::fill(t, Kind::Pool) };
        let round = CornerRadius::same((ring.width().min(ring.height()) / 2.0).min(255.0) as u8);
        painter.rect_filled(*ring, round, colour.gamma_multiply(0.12));
        painter.rect_stroke(*ring, round, Stroke::new(if lit { 3.0 } else { 2.0 }, colour), StrokeKind::Inside);
    }
    if let Some(pile) = places.pile {
        super::lab_pile::draw(ui, t, voice, state, pile);
    }
    let act = tokens_on(ui, t, voice, view, state, &places, (over_pool, pointer));
    marks(&painter, t, view, &places, now);
    if view.dragging.is_some() || !view.flights.is_empty() {
        ui.ctx().request_repaint();
    }
    act.or_else(|| background.clicked().then_some(MapAct::Select(None)))
}

/// Each token with its words, answering clicks and drags. Pools first: what is registered later
/// takes the pointer, so a miner inside a ring keeps its own clicks and drags.
fn tokens_on(
    ui: &mut Ui,
    t: &Tokens,
    voice: Voice,
    view: &mut MapView,
    state: &NetState,
    places: &Places,
    (over_pool, pointer): (Option<u32>, Option<Pos2>),
) -> Option<MapAct> {
    let painter = ui.painter().clone();
    let mut act = None;
    let pools = state.members.iter().filter(|member| member.kind == Kind::Pool);
    let others = state.members.iter().filter(|member| member.kind != Kind::Pool);
    for member in pools.chain(others) {
        let Some(centre) = places.at.get(&member.id).copied() else {
            continue;
        };
        let ring = places.rings.get(&member.id).copied();
        let words = Words::of(&painter, t, voice, member);
        let (shape, words_top) = match ring {
            Some(ring) => (Rect::from_center_size(ring.center_top(), vec2(BADGE, BADGE) * 2.0).union(ring), ring.center_bottom() + vec2(0.0, WORDS_GAP)),
            None => (Rect::from_center_size(centre, vec2(TOKEN, TOKEN) * 2.0), centre + vec2(0.0, TOKEN + WORDS_GAP)),
        };
        // The token and the words under it answer the hand alike, and are what a reader names.
        let hit = shape.union(words.rect(words_top));
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
        let selected = view.selected == Some(member.id);
        match ring {
            // The ring is the pool; its shape sits on the ring's top edge.
            Some(ring) => {
                let badge = ring.center_top();
                painter.circle_filled(badge, BADGE, glyphs::fill(t, Kind::Pool));
                glyphs::paint(&painter, Kind::Pool, badge, 16.0, glyphs::ink(t, Kind::Pool));
                if selected {
                    let round = CornerRadius::same((ring.width().min(ring.height()) / 2.0 + 3.0).min(255.0) as u8);
                    painter.rect_stroke(ring.expand(3.0), round, Stroke::new(2.5, t.info), StrokeKind::Outside);
                }
                words.paint(&painter, t, words_top);
            }
            None => {
                let ghost = (view.dragging == Some(member.id)).then_some(pointer).flatten();
                let drawn_at = ghost.unwrap_or(centre);
                token(&painter, t, member, drawn_at, selected || response.hovered());
                if selected {
                    painter.circle_stroke(drawn_at, TOKEN + 5.0, Stroke::new(2.5, t.info));
                }
                words.paint(&painter, t, drawn_at + vec2(0.0, TOKEN + WORDS_GAP));
            }
        }
    }
    act
}

fn token(painter: &Painter, t: &Tokens, member: &Member, centre: Pos2, lifted: bool) {
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
    // Shares are drawn as dots though no sentence tells of them.
    let drawn = |what: &What| matches!(what, What::Shares { .. }) || super::lab_events::told(state, what);
    for event in fresh.iter().filter(|event| !matches!(event.what, What::Block { .. }) && drawn(&event.what)) {
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
        _ => MARK_SECONDS,
    }
}

/// What moves, and the short mark a moment leaves on its node: its words are under the map.
fn marks(painter: &Painter, t: &Tokens, view: &MapView, places: &Places, now: f64) {
    let spot = |id: u32| places.at.get(&id).copied();
    // A ring that fades out and a badge on the token's shoulder, both inside its cell.
    let mark = |at: Pos2, colour: Color32, part: f32, badge: &dyn Fn(Pos2)| {
        let fade = (1.0 - part).clamp(0.0, 1.0);
        painter.circle_stroke(at, TOKEN + 4.0 + part * 6.0, Stroke::new(2.5, colour.gamma_multiply(fade)));
        let shoulder = at + vec2(TOKEN * 0.75, -TOKEN * 0.75);
        painter.circle_filled(shoulder, 8.0, t.card_top);
        badge(shoulder);
    };
    for flight in &view.flights {
        let part = ((now - flight.born) / life(&flight.what)) as f32;
        match &flight.what {
            What::Block { finder: Some(finder), .. } => {
                let Some(from) = spot(*finder) else {
                    continue;
                };
                let colour = if places.rings.contains_key(finder) { t.soft } else { t.text };
                painter.circle_stroke(from, TOKEN + part * 10.0, Stroke::new(2.5, colour.gamma_multiply(1.0 - part)));
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
                    let count = blocks.to_string();
                    mark(at, t.warn, part, &|badge| {
                        painter.text(badge, Align2::CENTER_CENTER, &count, FontId::proportional(10.5), t.warn);
                    });
                }
            }
            // A refusal is the network doing its work: a quiet bar, never the attacker's red.
            What::Refused { by, .. } => {
                if let Some(at) = spot(*by) {
                    mark(at, t.soft, part, &|badge| {
                        painter.circle_stroke(badge, 5.5, Stroke::new(1.5, t.soft));
                        painter.line_segment([badge - vec2(3.5, 0.0), badge + vec2(3.5, 0.0)], Stroke::new(1.5, t.soft));
                    });
                }
            }
            What::Settled { at, .. } => {
                if let Some(at) = spot(*at) {
                    mark(at, t.good, part, &|badge| glyphs::check(painter, badge, 11.0, t.good));
                }
            }
            What::Undone { at, .. } => {
                if let Some(at) = spot(*at) {
                    mark(at, t.warn, part, &|badge| glyphs::cross(painter, badge, 11.0, t.warn));
                }
            }
            _ => {}
        }
    }
}
