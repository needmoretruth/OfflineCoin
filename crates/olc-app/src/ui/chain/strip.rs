//! The latest blocks as lit boxes, newest on the left, and a branch beside them as a second row.
use super::{ago, faces, miner_name, Faces, Look};
use crate::core::{ChainView, MinedBlock, Miner};
use crate::i18n::{self, Voice};
use crate::ui::{fonts, format, paint, tokens};
use egui::text::{LayoutJob, TextWrapping};
use egui::{pos2, vec2, Align2, Color32, FontFamily, FontId, Painter, Pos2, Rect, Response, Sense, Shape, Stroke, Ui};

/// How long a new block takes to slide in.
const SLIDE_SECONDS: f64 = 0.4;
/// The face of a block, in points, when the caller does not say.
pub const BLOCK: f32 = 84.0;
const GAP: f32 = 22.0;
const LABEL: f32 = 22.0;
const CAPTION: f32 = 26.0;
/// How much of its face a dropped block keeps.
const DROPPED: f32 = 0.42;

/// What a second row stands for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BranchKind<'a> {
    /// A branch this node holds that races the top right now.
    Racing,
    /// Blocks that left the best chain when another branch won.
    Replaced,
    /// Anything else, such as the Lab's hidden chain, with the caller's own line.
    Told(&'a str),
}

/// The blocks to draw. `blocks` is the main row, newest first; `branch` sits under the heights it
/// shares with it, in any order.
pub struct Strip<'a> {
    pub blocks: &'a [MinedBlock],
    pub branch: Option<(&'a [MinedBlock], BranchKind<'a>)>,
    /// The side of a block's face.
    pub size: f32,
    /// Tells two strips on one page apart, for the slide.
    pub salt: &'a str,
}

impl<'a> Strip<'a> {
    /// The node's latest blocks with the open race, or else the last reorg.
    pub fn of(chain: &'a ChainView) -> Self {
        let racing = chain.rival.as_ref().map(|branch| (branch.blocks.as_slice(), BranchKind::Racing));
        let replaced = chain.replaced.as_ref().map(|branch| (branch.blocks.as_slice(), BranchKind::Replaced));
        Self { blocks: &chain.latest, branch: racing.or(replaced), size: BLOCK, salt: "chain" }
    }

    pub fn size(self, size: f32) -> Self {
        Self { size, ..self }
    }
}

fn depth(size: f32) -> f32 {
    (size * 0.17).round()
}

fn row_height(size: f32) -> f32 {
    LABEL + depth(size) + size + LABEL + 4.0
}

/// One line of text cut to `width`, with an ellipsis when it does not fit.
fn one_line(ui: &Ui, text: String, font: FontId, colour: Color32, width: f32) -> std::sync::Arc<egui::Galley> {
    let mut job = LayoutJob::simple_singleline(text, font, colour);
    job.wrap = TextWrapping { max_width: width, max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
    ui.fonts_mut(|fonts| fonts.layout_job(job))
}

fn draw_box(painter: &Painter, front: Rect, d: f32, look: &Faces, fade: f32) {
    let shade = |colour: Color32| colour.gamma_multiply(fade);
    let up = vec2(-d, -d);
    let top = vec![front.left_top(), front.right_top(), front.right_top() + up, front.left_top() + up];
    let side = vec![front.left_top() + up, front.left_top(), front.left_bottom(), front.left_bottom() + up];
    painter.add(Shape::convex_polygon(side, shade(look.side), Stroke::NONE));
    painter.add(Shape::convex_polygon(top, shade(look.top), Stroke::NONE));
    painter.add(paint::gradient(front, 2.0, shade(look.front_top), shade(look.front_bottom)));
    // The edge the light catches, between the top and the front.
    let edge = shade(super::mix(look.top, look.ink, 0.1));
    painter.line_segment([front.left_top(), front.right_top()], Stroke::new(1.0, edge));
}

fn details(look: &Look, block: &MinedBlock) -> String {
    let voice = look.voice;
    let height = format::number(block.height.into());
    let lines = [
        i18n::fill(i18n::text(voice, "chain.block_at"), &[&height]),
        i18n::fill(i18n::text(voice, "chain.found_by"), &[&miner_name(voice, &block.miner)]),
        i18n::fill(i18n::text(voice, "chain.id"), &[&block.short_hash()]),
        i18n::fill(i18n::text(voice, "chain.payments"), &[&format::number(block.payments as u128)]),
        i18n::fill(i18n::text(voice, "chain.reward"), &[&format::coins(block.reward, look.ticker)]),
        ago(voice, look.now_ms, block.time_ms),
    ];
    lines.join("\n")
}

/// Where the main row sits, eased from one slot to the right while a new block slides in.
fn slide(ui: &Ui, look: &Look, salt: &str, newest: Option<&MinedBlock>) -> f32 {
    let id = ui.scope_id().with(("chain-slide", salt));
    let now = ui.input(|input| input.time);
    let Some(newest) = newest else {
        return 1.0;
    };
    let seen: Option<(u64, [u8; 32], f64)> = ui.data(|data| data.get_temp(id));
    let start = match seen {
        Some((_, id_seen, start)) if id_seen == newest.id => start,
        Some((height, _, _)) if height + 1 == newest.height && !look.still => now,
        _ => f64::NEG_INFINITY,
    };
    ui.data_mut(|data| data.insert_temp(id, (newest.height, newest.id, start)));
    let part = ((now - start) / SLIDE_SECONDS).clamp(0.0, 1.0) as f32;
    if part < 1.0 {
        ui.ctx().request_repaint();
    }
    1.0 - (1.0 - part).powi(3)
}

struct Place<'p> {
    painter: &'p Painter,
    size: f32,
    left: f32,
    top_height: u64,
    shift: f32,
}

impl Place<'_> {
    fn front(&self, height: u64, y: f32) -> Rect {
        let column = self.top_height.saturating_sub(height) as f32;
        let x = self.left + depth(self.size) + column * (self.size + GAP) + self.shift;
        Rect::from_min_size(pos2(x, y + LABEL + depth(self.size)), vec2(self.size, self.size))
    }
}

fn draw_block(ui: &mut Ui, look: &Look, place: &Place, front: Rect, block: &MinedBlock, fade: f32) {
    let you = block.miner == Miner::You;
    let colours = faces(look.t, you);
    let d = depth(place.size);
    draw_box(place.painter, front, d, &colours, fade);
    let face = FontId::new((place.size * 0.2).clamp(12.0, 22.0), fonts::strong());
    let when = one_line(ui, ago(look.voice, look.now_ms, block.time_ms), face, colours.ink.gamma_multiply(fade), place.size - 8.0);
    place.painter.galley(front.center() - when.size() / 2.0, when, colours.ink);
    let height = format::number(block.height.into());
    let above = pos2(front.center().x - d / 2.0, front.top() - d - 4.0);
    let font = FontId::new(tokens::META, fonts::medium());
    place.painter.text(above, Align2::CENTER_BOTTOM, height, font.clone(), colours.label.gamma_multiply(fade));
    let name_colour = if you { look.t.info } else { look.t.text }.gamma_multiply(fade);
    let name = one_line(ui, miner_name(look.voice, &block.miner), font, name_colour, place.size + GAP - 4.0);
    let below = pos2(front.center().x - name.size().x / 2.0, front.bottom() + 6.0);
    place.painter.galley(below, name, name_colour);
    let whole = Rect::from_min_max(front.left_top() - vec2(d, d), front.right_bottom());
    let response = ui.interact(whole, ui.scope_id().with(("chain-block", block.id)), Sense::hover());
    let said = details(look, block);
    let label = said.lines().take(2).collect::<Vec<_>>().join(", ");
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, &label));
    response.on_hover_text(said);
}

fn caption(voice: Voice, branch: &[MinedBlock], kind: BranchKind) -> String {
    let heights = branch.iter().map(|block| block.height);
    let (first, last) = (heights.clone().min().unwrap_or(0), heights.max().unwrap_or(0));
    let (first, last) = (format::number(first.into()), format::number(last.into()));
    match kind {
        BranchKind::Racing => i18n::fill(i18n::text(voice, "chain.racing"), &[&last]),
        BranchKind::Replaced if first == last => i18n::fill(i18n::text(voice, "chain.replaced_one"), &[&first]),
        BranchKind::Replaced => i18n::fill(i18n::text(voice, "chain.replaced"), &[&first, &last]),
        BranchKind::Told(line) => line.to_string(),
    }
}

/// Draws the strip across the width it is given and as tall as its rows need.
pub fn block_strip(ui: &mut Ui, look: &Look, strip: &Strip) -> Response {
    let size = strip.size;
    let width = ui.available_width();
    let fits = ((width - depth(size)) / (size + GAP)).floor().max(1.0) as u64;
    let branch = strip.branch.filter(|(blocks, _)| !blocks.is_empty());
    let branch_top = branch.and_then(|(blocks, _)| blocks.iter().map(|block| block.height).max()).unwrap_or(0);
    let top_height = strip.blocks.first().map_or(0, |newest| newest.height).max(branch_top);
    let in_view = |block: &&MinedBlock| top_height.saturating_sub(block.height) < fits;
    let shown: Vec<&MinedBlock> = branch.map(|(blocks, _)| blocks.iter().filter(in_view).collect()).unwrap_or_default();
    // A branch below the blocks in view keeps its line and gives up its row.
    let second = if shown.is_empty() { 0.0 } else { row_height(size) };
    let height = row_height(size) + second + if branch.is_some() { CAPTION } else { 0.0 };
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    let Some(newest) = strip.blocks.first() else {
        let font = FontId::new(tokens::TEXT, FontFamily::Proportional);
        let empty = i18n::text(look.voice, "chain.no_blocks");
        ui.painter().text(rect.left_center(), Align2::LEFT_CENTER, empty, font, look.t.muted);
        return response;
    };
    let painter = ui.painter_at(rect);
    let eased = slide(ui, look, strip.salt, Some(newest));
    let place = Place { painter: &painter, size, left: rect.left(), top_height, shift: -(1.0 - eased) * (size + GAP) };
    // While sliding, one more block leaves on the right; at rest only whole blocks show.
    let reach = if eased < 1.0 { fits } else { fits - 1 };
    for block in strip.blocks.iter().filter(|block| top_height.saturating_sub(block.height) <= reach) {
        let front = place.front(block.height, rect.top());
        draw_block(ui, look, &place, front, block, 1.0);
    }
    let Some((blocks, kind)) = branch else {
        return response;
    };
    let y = rect.top() + row_height(size);
    let fade = if kind == BranchKind::Replaced { DROPPED } else { 1.0 };
    for block in &shown {
        let front = place.front(block.height, y);
        draw_block(ui, look, &place, front, block, fade);
    }
    // The fork: from the block both rows share down to the first block of the branch, when that
    // block is in view.
    let first = shown.iter().min_by_key(|block| block.height);
    let parent_shown = |first: &&&MinedBlock| {
        let parent = first.height.saturating_sub(1);
        top_height.saturating_sub(parent) < fits && strip.blocks.iter().any(|block| block.height == parent)
    };
    if let Some(first) = first.filter(parent_shown) {
        let parent = place.front(first.height.saturating_sub(1), rect.top());
        let child = place.front(first.height, y);
        let stroke = Stroke::new(1.5, look.t.line_strong);
        let elbow = pos2(parent.center().x, child.center().y);
        let reach = Pos2::new(child.right() + 2.0, child.center().y);
        painter.line_segment([pos2(parent.center().x, parent.bottom() + LABEL + 2.0), elbow], stroke);
        painter.line_segment([elbow, reach], stroke);
    }
    let line = caption(look.voice, blocks, kind);
    let font = FontId::new(tokens::META, FontFamily::Proportional);
    let at = pos2(rect.left() + depth(size), rect.top() + row_height(size) + second + 4.0);
    painter.text(at, Align2::LEFT_TOP, &line, font, look.t.soft);
    let said = ui.interact(Rect::from_min_size(at, vec2(width, CAPTION)), ui.scope_id().with(("chain-caption", strip.salt)), Sense::hover());
    said.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, &line));
    response
}
