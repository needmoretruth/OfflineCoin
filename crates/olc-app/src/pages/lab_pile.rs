//! The payments waiting for a block, as a pile on the map: a small square for each, those the next
//! block takes in the colour of a block and the rest in yellow, and how full the next block would
//! be against the most a block may hold.
use super::lab_tokens::number;
use crate::i18n::{fill, text, Voice};
use crate::local::lab_net::{Kind, Member, NetState};
use crate::ui::tokens::Tokens;
use egui::{vec2, Align2, CornerRadius, FontId, Rect, Sense, Ui};

const SQUARE: f32 = 7.0;
const SQUARE_GAP: f32 = 2.0;
const SQUARE_ROWS: usize = 3;

/// The participant whose waiting list the pile shows: the one furthest along that keeps blocks
/// and is not the attacker, whose chain is hidden while it attacks.
pub fn shown(state: &NetState) -> Option<&Member> {
    state
        .members
        .iter()
        .filter(|member| !matches!(member.kind, Kind::Attacker | Kind::Light))
        .filter(|member| member.status.as_ref().is_some_and(|status| !status.chain.is_empty()))
        .max_by_key(|member| member.height())
}

/// Payments waiting, their bytes, and the most bytes a block holds.
pub fn waiting(state: &NetState) -> (usize, u64, u64) {
    let Some(status) = shown(state).and_then(|member| member.status.as_ref()) else {
        return (0, 0, state.max_block_bytes);
    };
    let line = status.lab.clone().unwrap_or_default();
    let most = if line.max_block_bytes > 0 { line.max_block_bytes } else { state.max_block_bytes };
    (line.waiting as usize, line.waiting_bytes, most)
}

fn kb(bytes: u64) -> String {
    number(bytes.div_ceil(1024))
}

/// Draws the pile in `rect`, and names it for whoever reads the screen out.
pub fn draw(ui: &mut Ui, t: &Tokens, voice: Voice, state: &NetState, rect: Rect) {
    let (count, bytes, most) = waiting(state);
    let most = most.max(1);
    let next = bytes.min(most);
    // As many as fit the next block, by their share of the bytes.
    let fits = if bytes <= most { count } else { (count as u64 * most / bytes.max(1)) as usize };
    let title = fill(text(voice, "net.pile"), &[&number(count)]);
    let full = fill(text(voice, "net.pile_next"), &[&kb(next), &kb(most)]);
    let over = (count > fits).then(|| fill(text(voice, "net.pile_over"), &[&number(count - fits)]));
    let said = [Some(title.clone()), Some(full.clone()), over.clone()].into_iter().flatten().collect::<Vec<_>>().join(" ");
    let response = ui.interact(rect, ui.scope_id().with("lab-pile"), Sense::hover());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, &said));
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, CornerRadius::same(8), t.card_top.gamma_multiply(0.7));
    let inner = rect.shrink(8.0);
    painter.text(inner.left_top(), Align2::LEFT_TOP, &title, FontId::proportional(12.5), t.text);
    // What waits past the next block is a line of its own; each line wraps inside the box, the
    // bar sits right above them and the squares above that.
    let words = over.map_or(full.clone(), |over| format!("{full}\n{over}"));
    let colour = if bytes > most { t.warn } else { t.muted };
    let galley = painter.layout(words, FontId::proportional(11.5), colour, inner.width());
    let words_top = inner.bottom() - galley.size().y;
    let bar = Rect::from_min_size(egui::pos2(inner.left(), words_top - 9.0), vec2(inner.width(), 5.0));
    let top = inner.top() + 20.0;
    let rows = (((bar.top() - 4.0 - top + SQUARE_GAP) / (SQUARE + SQUARE_GAP)).floor().max(1.0) as usize).min(SQUARE_ROWS);
    let per_row = ((inner.width() + SQUARE_GAP) / (SQUARE + SQUARE_GAP)).floor().max(1.0) as usize;
    let shown = count.min(per_row * rows);
    for index in 0..shown {
        let (column, row) = (index % per_row, index / per_row);
        let at = vec2(column as f32 * (SQUARE + SQUARE_GAP), row as f32 * (SQUARE + SQUARE_GAP));
        let square = Rect::from_min_size(egui::pos2(inner.left(), top) + at, vec2(SQUARE, SQUARE));
        let colour = if index < fits { t.soft } else { t.warn };
        painter.rect_filled(square, CornerRadius::same(1), colour);
    }
    painter.rect_filled(bar, CornerRadius::same(2), t.hover);
    let part = next as f32 / most as f32;
    let lit = Rect::from_min_size(bar.min, vec2(bar.width() * part, bar.height()));
    painter.rect_filled(lit, CornerRadius::same(2), if bytes > most { t.warn } else { t.soft });
    painter.galley(egui::pos2(inner.left(), words_top), galley, colour);
}
