//! The network's newest blocks under the map: each block filled as far as its payments fill it,
//! a bad miner's blocks in red, the blocks a chain with more work replaced struck through in
//! yellow, how much waits for a block, and buttons that make blocks at once.
use crate::i18n::{fill, text, Voice};
use crate::local::lab_net::{Kind, Member, NetState, Order};
use crate::ui::tokens::{self, Tokens};
use crate::ui::widgets::{button_on, Kind as ButtonKind};
use egui::{vec2, Align2, Color32, CornerRadius, FontId, Rect, RichText, Sense, Stroke, StrokeKind, Ui};

const BLOCK: f32 = 30.0;
const BLOCK_HIGH: f32 = 36.0;
const GAP: f32 = 6.0;
/// The presses that make blocks at once.
const MAKE: [u64; 3] = [10, 100, 300];
const SHOWN_REPLACED: usize = 6;

/// The participant whose chain the strip shows: the one furthest along that keeps blocks and is
/// not the attacker, whose chain is hidden while it attacks.
fn shown(state: &NetState) -> Option<&Member> {
    state
        .members
        .iter()
        .filter(|member| !matches!(member.kind, Kind::Attacker | Kind::Light))
        .filter(|member| member.status.as_ref().is_some_and(|status| !status.chain.is_empty()))
        .max_by_key(|member| member.height())
}

fn tile(ui: &mut Ui, t: &Tokens, colour: Color32, full: f32, struck: bool, label: &str) {
    let (rect, _) = ui.allocate_exact_size(vec2(BLOCK, BLOCK_HIGH + 14.0), Sense::hover());
    let block = Rect::from_min_size(rect.min, vec2(BLOCK, BLOCK_HIGH));
    let painter = ui.painter();
    let round = CornerRadius::same(4);
    painter.rect_filled(block, round, t.hover);
    let height = (BLOCK_HIGH - 4.0) * full.clamp(0.0, 1.0);
    if height > 0.5 {
        let inner = Rect::from_min_max(block.left_bottom() + vec2(2.0, -2.0 - height), block.right_bottom() + vec2(-2.0, -2.0));
        painter.rect_filled(inner, CornerRadius::same(2), colour.gamma_multiply(0.75));
    }
    let edge = if struck { t.warn } else { colour };
    painter.rect_stroke(block, round, Stroke::new(1.5, edge), StrokeKind::Inside);
    if struck {
        painter.line_segment([block.left_bottom(), block.right_top()], Stroke::new(1.5, t.warn));
    }
    painter.text(rect.center_bottom(), Align2::CENTER_BOTTOM, label, FontId::proportional(10.5), t.muted);
}

/// The strip. Returns the number of blocks to make when a press asked for them.
pub fn draw(ui: &mut Ui, t: &Tokens, voice: Voice, state: &NetState) -> Option<Order> {
    let member = shown(state);
    let mut order = None;
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
    let attackers: Vec<&str> =
        state.members.iter().filter(|other| other.kind == Kind::Attacker).filter_map(Member::address).collect();
    let most = line.max_block_bytes.max(1) as f32;
    let room = ((ui.available_width() + GAP) / (BLOCK + GAP)).floor().max(4.0) as usize;
    let replaced: Vec<&String> = state.replaced.iter().rev().take(SHOWN_REPLACED).collect();
    let fits = room.saturating_sub(if replaced.is_empty() { 0 } else { replaced.len().min(room / 3) + 1 });
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing = vec2(GAP, GAP);
        let blocks = &status.chain[status.chain.len().saturating_sub(fits)..];
        for block in blocks {
            let bytes = line.sizes.iter().find(|(height, ..)| *height == block.height).map_or(0, |(_, bytes, _)| *bytes);
            let bad = attackers.contains(&block.miner.as_str());
            let colour = if bad { t.bad } else { t.soft };
            tile(ui, t, colour, bytes as f32 / most, false, &block.height.to_string());
        }
        if !replaced.is_empty() {
            ui.add_space(GAP * 2.0);
            // A block's id says something only to who reads ids.
            let expert = voice.mode == crate::settings::Mode::Expert;
            for id in replaced.iter().take(room / 3) {
                let label: String = if expert { id.chars().take(4).collect() } else { String::new() };
                tile(ui, t, t.warn, 0.0, true, &label);
            }
        }
    });
    let waiting_kb = line.waiting_bytes.div_ceil(1024);
    let colour = if line.waiting_bytes > line.max_block_bytes { t.warn } else { t.muted };
    let words = fill(text(voice, "net.waiting"), &[&waiting_kb.to_string(), &(line.max_block_bytes / 1024).to_string()]);
    ui.label(RichText::new(words).color(colour).size(tokens::META));
    if !replaced.is_empty() || !attackers.is_empty() {
        ui.horizontal_wrapped(|ui| {
            legend(ui, t, t.soft, text(voice, "net.legend_fill"));
            if !attackers.is_empty() {
                legend(ui, t, t.bad, text(voice, "net.legend_bad"));
            }
            if !replaced.is_empty() {
                legend(ui, t, t.warn, text(voice, "net.legend_replaced"));
            }
        });
    }
    order = order.or(make(ui, t, voice, state));
    order
}

fn legend(ui: &mut Ui, t: &Tokens, colour: Color32, words: &str) {
    let (rect, _) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
    ui.painter().rect_stroke(rect, CornerRadius::same(2), Stroke::new(1.5, colour), StrokeKind::Inside);
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
