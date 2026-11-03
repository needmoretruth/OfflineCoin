//! Each miner's share of the latest blocks, as bars side by side.
use super::{faces, miner_name, Look};
use crate::core::{Miner, Share};
use crate::i18n;
use crate::ui::{fonts, format, paint, tokens};
use egui::{pos2, vec2, Align2, FontFamily, FontId, Rect, Response, Sense, Ui};

const ROW: f32 = 34.0;
const BAR: f32 = 10.0;
const NAME: f32 = 132.0;
const FIGURES: f32 = 260.0;
/// The percent and the block count alone, when the speed leaves the row.
const FIGURES_NARROW: f32 = 140.0;
/// Below this the bars are too short to compare, so a narrow column drops the speed instead.
const MIN_TRACK: f32 = 110.0;

/// One bar per miner, most blocks first as the view already sorts them, with "you" in the accent.
/// A miner that hashes and has found nothing yet keeps an empty bar. In a narrow column the speed
/// leaves the row, so the bars keep room and no figure is cut at the edge; the hover still says it.
pub fn race_bars(ui: &mut Ui, look: &Look, shares: &[Share]) -> Response {
    let window: u32 = shares.iter().map(|share| share.blocks).sum();
    let t = look.t;
    let voice = look.voice;
    let width = ui.available_width();
    let rows = shares.len().max(1) as f32;
    let (rect, response) = ui.allocate_exact_size(vec2(width, tokens::META_LINE + 6.0 + rows * ROW), Sense::hover());
    let painter = ui.painter_at(rect);
    let small = FontId::new(tokens::META, FontFamily::Proportional);
    let title = i18n::fill(i18n::text(voice, "chain.shares"), &[&format::number(window.into())]);
    painter.text(rect.left_top(), Align2::LEFT_TOP, &title, small.clone(), t.soft);
    if window == 0 && shares.is_empty() {
        let y = rect.top() + tokens::META_LINE + 6.0 + ROW / 2.0;
        painter.text(pos2(rect.left(), y), Align2::LEFT_CENTER, i18n::text(voice, "chain.no_share"), small, t.muted);
        return response;
    }
    let most = shares.iter().map(|share| share.blocks).max().unwrap_or(1).max(1);
    let track_left = rect.left() + NAME;
    let with_speed = width - NAME - FIGURES >= MIN_TRACK;
    let figures = if with_speed { FIGURES } else { FIGURES_NARROW };
    let track_width = (width - NAME - figures).max(40.0);
    for (index, share) in shares.iter().enumerate() {
        let you = share.miner == Miner::You;
        let colours = faces(t, you);
        let middle = rect.top() + tokens::META_LINE + 6.0 + ROW * (index as f32 + 0.5);
        let name = miner_name(voice, &share.miner);
        let name_font = FontId::new(tokens::TEXT, if you { fonts::strong() } else { fonts::medium() });
        let name_colour = if you { t.info } else { t.text };
        let mut job = egui::text::LayoutJob::simple_singleline(name.clone(), name_font, name_colour);
        job.wrap = egui::text::TextWrapping { max_width: NAME - 12.0, max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
        let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
        painter.galley(pos2(rect.left(), middle - galley.size().y / 2.0), galley, name_colour);
        let track = Rect::from_min_size(pos2(track_left, middle - BAR / 2.0), vec2(track_width, BAR));
        painter.rect_filled(track, BAR / 2.0, t.hover);
        // Bars are measured against the leader, so the race between the first two reads at a glance.
        let part = share.blocks as f32 / most as f32;
        if share.blocks > 0 {
            let fill = Rect::from_min_size(track.min, vec2((track_width * part).max(BAR), BAR));
            let (start, end) = colours.bar(t);
            painter.add(paint::gradient(fill, BAR / 2.0, start, end));
        }
        let percent = (share.blocks * 100).checked_div(window).unwrap_or(0);
        let count = i18n::fill(i18n::text(voice, "chain.share_blocks"), &[&format::number(share.blocks.into())]);
        let figure_left = track.right() + 14.0;
        let big = FontId::new(tokens::SECTION, fonts::strong());
        let shown = painter.text(pos2(figure_left, middle), Align2::LEFT_CENTER, format!("{percent}%"), big, name_colour);
        let counted = painter.text(pos2(shown.right() + 8.0, middle), Align2::LEFT_CENTER, &count, small.clone(), t.soft);
        let speed = share.hashrate.map(|rate| format::rate(voice, rate));
        if let Some(speed) = speed.as_ref().filter(|_| with_speed) {
            painter.text(pos2(counted.right() + 12.0, middle), Align2::LEFT_CENTER, speed, small.clone(), t.muted);
        }
        let row = Rect::from_min_size(pos2(rect.left(), middle - ROW / 2.0), vec2(width, ROW));
        let label = [Some(name), Some(format!("{percent}%")), Some(count), speed].into_iter().flatten().collect::<Vec<_>>().join(", ");
        let said = ui.interact(row, ui.scope_id().with(("chain-share", index)), Sense::hover());
        said.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, &label));
        said.on_hover_text(label);
    }
    response
}
