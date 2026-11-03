//! The two chains of a Lab run as rows of lit blocks, one above the other. Blocks at the same
//! height stand in the same column, so the moment the rows part and the race between them can be
//! seen. Each block has the light and shadow of the block strip, so both read as the same thing.
use crate::i18n::{fill, text, Voice};
use crate::local::lab::{LabKind, Mark, Picture, Row, Side};
use crate::ui::chain;
use crate::ui::paint;
use crate::ui::tokens::{self, Tokens};
use egui::epaint::Stroke;
use egui::{pos2, vec2, Align2, Color32, FontId, Rect, Sense, Ui};

/// Sizes of one block: the live strip and the small one beside each step.
#[derive(Clone, Copy)]
pub struct Scale {
    pub width: f32,
    pub height: f32,
    pub gap: f32,
    pub numbers: bool,
}

pub const LIVE: Scale = Scale { width: 46.0, height: 32.0, gap: 6.0, numbers: true };
pub const SMALL: Scale = Scale { width: 22.0, height: 15.0, gap: 3.0, numbers: false };
/// Empty places a row shows before its first block.
const EMPTY_SLOTS: usize = 10;
/// How strongly an empty place shows: the outline of a block to come.
const EMPTY_FADE: f32 = 0.22;

/// The colour of a block by whose reward it holds. The attacker's side takes the colour of harm and
/// the side that splits off a warning; everyone else is the strip's gray, since its accent stands
/// for the person alone.
pub fn side_colour(t: &Tokens, kind: LabKind, side: Side) -> Color32 {
    match (kind, side) {
        (LabKind::Attack, Side::Strong) => t.bad,
        (LabKind::Split, Side::Strong) => t.warn,
        (_, Side::Other | Side::Unknown) => chain::gray(t),
    }
}

/// The lit faces of a block in this colour.
fn faces(t: &Tokens, colour: Color32) -> chain::Faces {
    chain::faces_in(t, colour, t.text, t.soft)
}

/// One lit block in `rect`, its top and side rising inside it, with a mark for the payment or the
/// secret spend on its face.
fn block(ui: &Ui, t: &Tokens, rect: Rect, colour: Color32, mark: &Mark, faint: bool, scale: Scale) {
    let painter = ui.painter();
    let alpha = if faint { 0.32 } else { 1.0 };
    let depth = chain::depth(scale.height);
    let face = Rect::from_min_max(rect.min + vec2(depth, depth), rect.max);
    chain::draw_box(painter, face, depth, &faces(t, colour), alpha);
    let centre = face.center();
    let dot = scale.height * 0.17;
    if mark.payment {
        painter.circle(centre, dot, t.warn.gamma_multiply(alpha), Stroke::new(1.5, t.text.gamma_multiply(alpha)));
    }
    if mark.spend {
        let reach = dot * 1.3;
        let stroke = Stroke::new(if scale.numbers { 2.5 } else { 1.5 }, t.text.gamma_multiply(alpha));
        painter.line_segment([centre + vec2(-reach, -reach), centre + vec2(reach, reach)], stroke);
        painter.line_segment([centre + vec2(-reach, reach), centre + vec2(reach, -reach)], stroke);
    }
}

/// The first and last height the rows show, so both line up by height.
pub fn span(rows: &[&[Mark]], fits: usize) -> Option<(u64, u64)> {
    let top = rows.iter().flat_map(|row| row.iter()).map(|mark| mark.height).max()?;
    let low = rows.iter().flat_map(|row| row.iter()).map(|mark| mark.height).min()?;
    let fits = u64::try_from(fits.max(1)).unwrap_or(1);
    Some((low.max((top + 1).saturating_sub(fits)), top))
}

/// One row of blocks from `first` to `last` height. `shared` blocks are drawn faint: the same
/// block sits on the other row too. Replaced blocks are struck through.
/// Where a row's columns start and end, and how big each block is.
#[derive(Clone, Copy)]
struct Columns {
    kind: LabKind,
    span: (u64, u64),
    scale: Scale,
}

fn blocks(ui: &mut Ui, t: &Tokens, columns: Columns, marks: &[Mark], shared: &[Mark], struck: bool) {
    let Columns { kind, span, scale } = columns;
    let count = span.1.saturating_sub(span.0) + 1;
    let width = count as f32 * (scale.width + scale.gap);
    let label = if scale.numbers { 16.0 } else { 0.0 };
    let (rect, _) = ui.allocate_exact_size(vec2(width, scale.height + label), Sense::hover());
    for mark in marks.iter().filter(|mark| (span.0..=span.1).contains(&mark.height)) {
        let column = (mark.height - span.0) as f32;
        let left = rect.left() + column * (scale.width + scale.gap);
        let place = Rect::from_min_size(pos2(left, rect.top()), vec2(scale.width, scale.height));
        let same = shared.iter().any(|other| other.height == mark.height && other.id == mark.id);
        block(ui, t, place, side_colour(t, kind, mark.side), mark, same || struck, scale);
        if struck {
            let stroke = Stroke::new(2.0, t.soft);
            let middle = place.center().y + chain::depth(scale.height) / 2.0;
            ui.painter().line_segment([pos2(place.left(), middle), pos2(place.right(), middle)], stroke);
        }
        if scale.numbers {
            ui.painter().text(
                pos2(place.center().x, place.bottom() + 2.0),
                Align2::CENTER_TOP,
                format!("#{}", mark.height),
                FontId::proportional(11.0),
                t.muted,
            );
        }
    }
}

/// A bar as long as the side's share of all hashing, filled as the race bars are.
fn share_bar(ui: &mut Ui, t: &Tokens, share: f32, colour: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(120.0, 8.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 4.0, t.hover);
    let filled = Rect::from_min_size(rect.min, vec2(rect.width() * share.clamp(0.0, 1.0), rect.height()));
    let (start, end) = faces(t, colour).bar(t);
    painter.add(paint::gradient(filled, 4.0, start, end));
}

/// The words over each row: whose chain it is, its share, and whether it is cut off.
fn heading(ui: &mut Ui, t: &Tokens, voice: Voice, kind: LabKind, row: &Row, upper: bool) {
    let (name, side) = match (kind, upper) {
        (LabKind::Attack, true) => ("lab2.public", Side::Other),
        (LabKind::Attack, false) => ("lab2.hidden", Side::Strong),
        (LabKind::Split, true) => ("lab2.left", Side::Strong),
        (LabKind::Split, false) => ("lab2.right", Side::Other),
    };
    let colour = side_colour(t, kind, side);
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(text(voice, name)).strong().color(t.text));
        share_bar(ui, t, row.share, colour);
        let percent = format!("{:.0}", row.share * 100.0);
        let rate = crate::ui::format::number(row.rate.into());
        ui.label(egui::RichText::new(fill(text(voice, "lab2.row_share"), &[&percent, &rate])).color(t.soft).size(tokens::META));
        if row.apart {
            ui.label(egui::RichText::new(text(voice, "lab2.apart")).color(t.warn).size(tokens::META).strong());
        }
    });
}

/// How many blocks of `scale` fit across this width.
pub fn fits(width: f32, scale: Scale) -> usize {
    ((width / (scale.width + scale.gap)).floor() as usize).max(4)
}

/// The rows before a run, from what was chosen: each side's share of the hashing and no blocks yet.
pub fn planned(kind: LabKind, strong_percent: u8, total_rate: u64) -> Picture {
    let strong = f32::from(strong_percent.min(100)) / 100.0;
    let row = |share: f32| Row { blocks: Vec::new(), height: 0, rate: (total_rate as f32 * share).round() as u64, share, apart: false };
    let (upper, lower) = match kind {
        LabKind::Attack => (row(1.0 - strong), row(strong)),
        LabKind::Split => (row(strong), row(1.0 - strong)),
    };
    Picture { upper, lower, replaced: Vec::new() }
}

/// Faint blocks where blocks will stand, for rows that have none yet.
fn empty(ui: &mut Ui, t: &Tokens, count: usize, scale: Scale) {
    let width = count as f32 * (scale.width + scale.gap);
    let (rect, _) = ui.allocate_exact_size(vec2(width, scale.height), Sense::hover());
    let depth = chain::depth(scale.height);
    let look = faces(t, chain::gray(t));
    for column in 0..count {
        let left = rect.left() + column as f32 * (scale.width + scale.gap);
        let place = Rect::from_min_size(pos2(left, rect.top()), vec2(scale.width, scale.height));
        let face = Rect::from_min_max(place.min + vec2(depth, depth), place.max);
        chain::draw_box(ui.painter(), face, depth, &look, EMPTY_FADE);
    }
}

/// Both rows, and the replaced blocks under them when there are any. Before the first block the
/// rows stand empty with their shares, so a person sees what will race before it starts.
pub fn picture(ui: &mut Ui, t: &Tokens, voice: Voice, kind: LabKind, picture: &Picture, scale: Scale) {
    let fit = fits(ui.available_width(), scale);
    let rows = [picture.upper.blocks.as_slice(), picture.lower.blocks.as_slice(), picture.replaced.as_slice()];
    let Some(span) = span(&rows, fit) else {
        let slots = fit.min(EMPTY_SLOTS);
        if scale.numbers {
            heading(ui, t, voice, kind, &picture.upper, true);
        }
        empty(ui, t, slots, scale);
        ui.add_space(6.0);
        if scale.numbers {
            heading(ui, t, voice, kind, &picture.lower, false);
        }
        empty(ui, t, slots, scale);
        return;
    };
    let gap = if scale.numbers { 6.0 } else { 2.0 };
    let columns = Columns { kind, span, scale };
    if scale.numbers {
        heading(ui, t, voice, kind, &picture.upper, true);
    }
    blocks(ui, t, columns, &picture.upper.blocks, &[], false);
    ui.add_space(gap);
    if scale.numbers {
        heading(ui, t, voice, kind, &picture.lower, false);
    }
    blocks(ui, t, columns, &picture.lower.blocks, &picture.upper.blocks, false);
    if !picture.replaced.is_empty() {
        ui.add_space(gap);
        if scale.numbers {
            ui.label(egui::RichText::new(text(voice, "lab2.replaced")).color(t.soft).strong());
        }
        blocks(ui, t, columns, &picture.replaced, &[], true);
    }
}

/// A swatch and its meaning, for the legend.
pub fn swatch(ui: &mut Ui, t: &Tokens, colour: Color32, mark: &Mark, faint: bool, words: &str) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(SMALL.width + 6.0, SMALL.height + 4.0), Sense::hover());
        let scale = Scale { width: rect.width(), height: rect.height(), gap: 0.0, numbers: false };
        block(ui, t, rect, colour, mark, faint, scale);
        ui.label(egui::RichText::new(words).color(t.soft).size(tokens::META));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mark(height: u64) -> Mark {
        Mark { height, id: format!("{height:06}"), side: Side::Other, payment: false, spend: false }
    }

    #[test]
    fn rows_line_up_on_the_newest_heights_that_fit() {
        let upper = [mark(3), mark(4), mark(5)];
        let lower = [mark(3), mark(4), mark(5), mark(6), mark(7)];
        assert_eq!(span(&[&upper, &lower], 10), Some((3, 7)));
        assert_eq!(span(&[&upper, &lower], 3), Some((5, 7)));
        assert_eq!(span(&[&[], &[]], 3), None);
    }
}
