//! The network's newest blocks under the map, lit as the app's block strip draws them: each block
//! lit from the bottom as far as its payments fill it, a bad miner's in red, and under each its
//! height and who found it. The payments still waiting are the pile on the map, and the blocks a
//! chain with more work replaced are in the attack's own picture. Buttons make blocks at once.
use super::lab_pile::shown;
use super::lab_tokens::name;
use crate::i18n::{fill, text, Voice};
use crate::local::lab_net::{Kind, NetState, Order};
use crate::ui::chain;
use crate::ui::tokens::{self, Tokens};
use crate::ui::widgets::{button_on, Kind as ButtonKind};
use egui::text::{LayoutJob, TextWrapping};
use egui::{pos2, vec2, Align2, Color32, CornerRadius, FontId, Rect, RichText, Sense, Stroke, StrokeKind, Ui};

/// The face of a block, and the cell that holds it with its height and finder under it.
const FACE: f32 = 32.0;
const CELL: f32 = 76.0;
const GAP: f32 = 4.0;
/// How strongly the empty part of a block shows.
const EMPTY: f32 = 0.3;
/// The presses that make blocks at once.
const MAKE: [u64; 3] = [10, 100, 300];

/// One block in a cell: lit from the bottom up to `full`, with its height and finder under it.
fn block(ui: &mut Ui, t: &Tokens, colour: Color32, full: f32, height: u64, finder: &str) {
    let depth = chain::depth(FACE);
    let (cell, _) = ui.allocate_exact_size(vec2(CELL, depth + FACE + 30.0), Sense::hover());
    let painter = ui.painter_at(cell);
    let face = Rect::from_min_size(pos2(cell.center().x - FACE / 2.0 + depth / 2.0, cell.top() + depth), vec2(FACE, FACE));
    let look = chain::faces_in(t, colour, t.text, t.soft);
    chain::draw_box(&painter, face, depth, &look, EMPTY);
    let full = full.clamp(0.0, 1.0);
    if full > 0.0 {
        // A full block is lit to its top; a part full one only up its face and side.
        let top = if full >= 0.999 { face.top() - depth - 1.0 } else { face.bottom() - FACE * full };
        let lit = Rect::from_min_max(pos2(face.left() - depth - 1.0, top), face.max + vec2(1.0, 1.0));
        chain::draw_box(&painter.with_clip_rect(lit.intersect(cell)), face, depth, &look, 1.0);
    }
    let under = face.center_bottom() + vec2(-depth / 2.0, 3.0);
    painter.text(under, Align2::CENTER_TOP, format!("#{height}"), FontId::proportional(10.5), t.muted);
    let mut job = LayoutJob::simple_singleline(finder.to_string(), FontId::proportional(10.5), t.soft);
    job.wrap = TextWrapping { max_width: CELL - 4.0, max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
    let galley = painter.layout_job(job);
    painter.galley(under + vec2(-galley.size().x / 2.0, 14.0), galley, t.soft);
}

/// The strip. Returns the number of blocks to make when a press asked for them.
pub fn draw(ui: &mut Ui, t: &Tokens, voice: Voice, state: &NetState) -> Option<Order> {
    let member = shown(state);
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(text(voice, "net.blocks")).strong().color(t.text));
        if let Some(member) = member {
            ui.label(RichText::new(fill(text(voice, "net.height"), &[&member.height().to_string()])).color(t.muted));
        }
    });
    let Some(member) = member else {
        return make(ui, t, voice, state);
    };
    let status = member.status.clone().unwrap_or_default();
    let line = status.lab.clone().unwrap_or_default();
    let most = line.max_block_bytes.max(1) as f32;
    let fits = ((ui.available_width() + GAP) / (CELL + GAP)).floor().max(4.0) as usize;
    let finder = |miner: &str| state.members.iter().find(|other| other.address() == Some(miner));
    let mut bad = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing = vec2(GAP, GAP);
        for found in &status.chain[status.chain.len().saturating_sub(fits)..] {
            let bytes = line.sizes.iter().find(|(height, ..)| *height == found.height).map_or(0, |(_, bytes, _)| *bytes);
            let by = finder(&found.miner);
            let attacker = by.is_some_and(|other| other.kind == Kind::Attacker);
            bad |= attacker;
            let colour = if attacker { t.bad } else { chain::gray(t) };
            let named = by.map(|other| name(voice, other)).unwrap_or_default();
            block(ui, t, colour, bytes as f32 / most, found.height, &named);
        }
    });
    ui.horizontal_wrapped(|ui| {
        legend(ui, t, chain::gray(t), text(voice, "net.legend_fill"));
        if bad {
            legend(ui, t, t.bad, text(voice, "net.legend_bad"));
        }
    });
    make(ui, t, voice, state)
}

fn legend(ui: &mut Ui, t: &Tokens, colour: Color32, words: &str) {
    let (rect, _) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::same(2), colour);
    ui.painter().rect_stroke(rect, CornerRadius::same(2), Stroke::new(1.0, t.line), StrokeKind::Inside);
    ui.label(RichText::new(words).color(t.muted).size(tokens::META));
}

/// Presses that make 10, 100 or 300 blocks at once, and how far the making is.
fn make(ui: &mut Ui, t: &Tokens, voice: Voice, state: &NetState) -> Option<Order> {
    let miner = state.members.iter().any(|member| member.kind == Kind::Miner && member.status.is_some());
    if !miner {
        return None;
    }
    let mut order = None;
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(text(voice, "net.make")).color(t.soft));
        let free = state.making.is_none() && state.experiment.as_ref().is_none_or(|run| !matches!(run.phase, crate::local::lab_net::Phase::Running));
        for count in MAKE {
            if button_on(ui, t, &format!("+{count}"), ButtonKind::Quiet, free).clicked() {
                order = Some(Order::MakeBlocks(count));
            }
        }
        if let Some((made, asked)) = state.making {
            ui.label(RichText::new(fill(text(voice, "net.making"), &[&made.to_string(), &asked.to_string()])).color(t.text));
        }
    });
    order
}
