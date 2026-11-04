//! Each miner's share of the latest blocks, as bars side by side.
use super::{faces, miner_name, Look};
use crate::core::{Miner, Share};
use crate::i18n;
use crate::ui::{fonts, format, paint, tokens};
use egui::{pos2, vec2, Align2, FontFamily, FontId, Rect, Response, Sense, Ui};

const ROW: f32 = 34.0;
const BAR: f32 = 10.0;
/// Wide enough for "Pool" and a finder's short address, the widest name the app makes itself.
const NAME: f32 = 156.0;
const FIGURES: f32 = 260.0;
/// The percent and the block count alone, when the speed leaves the row.
const FIGURES_NARROW: f32 = 140.0;
/// Below this the bars are too short to compare, so a narrow column drops the speed instead.
const MIN_TRACK: f32 = 110.0;

/// Each miner's whole percent of the blocks, adding to 100 exactly: the floors first, then one
/// point each to the largest remainders, as a breakdown that has to add up is rounded.
pub fn percents(shares: &[Share]) -> Vec<u32> {
    let window: u32 = shares.iter().map(|share| share.blocks).sum();
    if window == 0 {
        return vec![0; shares.len()];
    }
    let mut given: Vec<u32> = shares.iter().map(|share| share.blocks * 100 / window).collect();
    let left = 100 - given.iter().sum::<u32>();
    let mut order: Vec<usize> = (0..shares.len()).collect();
    order.sort_by_key(|at| std::cmp::Reverse(shares[*at].blocks * 100 % window));
    for at in order.into_iter().take(left as usize) {
        given[at] += 1;
    }
    given
}

/// One bar per miner, most blocks first as the view already sorts them, with "you" in the accent.
/// A miner that hashes and has found nothing yet keeps an empty bar. In a narrow column the speed
/// leaves the row, so the bars keep room and no figure is cut at the edge; the hover still says it.
pub fn race_bars(ui: &mut Ui, look: &Look, shares: &[Share]) -> Response {
    let window: u32 = shares.iter().map(|share| share.blocks).sum();
    let t = look.t;
    let voice = look.voice;
    let width = ui.available_width();
    // A pool's row says under it which miners here work in it: their blocks are the pool's.
    let workers_of = |share: &Share| {
        let names: Vec<String> = share.workers.iter().map(|worker| miner_name(voice, worker)).collect();
        (!names.is_empty()).then(|| i18n::fill(i18n::text(voice, "chain.pool_workers"), &[&names.join(", ")]))
    };
    let height_of = |share: &Share| if share.workers.is_empty() { ROW } else { ROW + tokens::META_LINE };
    let rows: f32 = if shares.is_empty() { ROW } else { shares.iter().map(height_of).sum() };
    let (rect, response) = ui.allocate_exact_size(vec2(width, tokens::META_LINE + 6.0 + rows), Sense::hover());
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
    let rounded = percents(shares);
    let mut top = rect.top() + tokens::META_LINE + 6.0;
    for (index, share) in shares.iter().enumerate() {
        let you = share.miner == Miner::You;
        let colours = faces(t, you);
        let middle = top + ROW / 2.0;
        top += height_of(share);
        let workers = workers_of(share);
        if let Some(workers) = &workers {
            painter.text(pos2(rect.left(), middle + ROW / 2.0 - 4.0), Align2::LEFT_TOP, workers, small.clone(), t.muted);
        }
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
        let percent = rounded[index];
        let count = match share.blocks {
            1 => i18n::text(voice, "chain.share_one").to_string(),
            blocks => i18n::fill(i18n::text(voice, "chain.share_blocks"), &[&format::number(blocks.into())]),
        };
        let figure_left = track.right() + 14.0;
        let big = FontId::new(tokens::SECTION, fonts::strong());
        let shown = painter.text(pos2(figure_left, middle), Align2::LEFT_CENTER, format!("{percent}%"), big, name_colour);
        let counted = painter.text(pos2(shown.right() + 8.0, middle), Align2::LEFT_CENTER, &count, small.clone(), t.soft);
        let speed = share.hashrate.map(|rate| format::rate(voice, rate));
        if let Some(speed) = speed.as_ref().filter(|_| with_speed) {
            painter.text(pos2(counted.right() + 12.0, middle), Align2::LEFT_CENTER, speed, small.clone(), t.muted);
        }
        let row = Rect::from_min_size(pos2(rect.left(), middle - ROW / 2.0), vec2(width, height_of(share)));
        let whole = super::finder_address(&share.miner).map(str::to_string);
        let said = [Some(name), Some(format!("{percent}%")), Some(count), speed, workers, whole];
        let label = said.into_iter().flatten().collect::<Vec<_>>().join(", ");
        let said = ui.interact(row, ui.scope_id().with(("chain-share", index)), Sense::hover());
        said.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, &label));
        said.on_hover_text(label);
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Three equal miners read 34, 33 and 33, not 33 three times: the breakdown adds to 100.
    #[test]
    fn the_percents_add_to_a_hundred() {
        let of = |blocks: &[u32]| -> Vec<Share> { blocks.iter().map(|blocks| Share { blocks: *blocks, ..Share::default() }).collect() };
        assert_eq!(percents(&of(&[1, 1, 1])), [34, 33, 33]);
        assert_eq!(percents(&of(&[29, 17, 9, 4, 1])).iter().sum::<u32>(), 100);
        assert_eq!(percents(&of(&[0, 0])), [0, 0]);
    }

    /// Every finder the app names by an address, alone or as a pool, stands whole in the name
    /// column in both languages: one mark in the middle, never a second at the edge. Text size
    /// zooms the column with the words, so this holds at 150 % as well.
    #[test]
    fn a_finder_by_address_stands_whole_in_its_column() {
        let ctx = egui::Context::default();
        ctx.set_fonts(fonts::definitions());
        ctx.begin_pass(egui::RawInput::default());
        let mut cut = Vec::new();
        // An address of the owner's, and the widest and narrowest hex there is.
        let addresses = ["olc4ab6b30f0e9d2c7a1b5e8f3d6c9a2b4e7f0c3d6a9b2e5f8c1d4a7b0e3f6a9c2", &format!("olc{}", "0".repeat(40)), &format!("olc{}", "d".repeat(40))];
        for (lang, address) in [i18n::Lang::En, i18n::Lang::Ko].into_iter().flat_map(|lang| addresses.map(|address| (lang, address))) {
            for mode in [i18n::Mode::Basic, i18n::Mode::Expert] {
                let voice = i18n::Voice { lang, mode };
                for miner in [Miner::Away(address.into()), Miner::Pool(address.into()), Miner::OwnPool, Miner::Unknown, Miner::You] {
                    let name = miner_name(voice, &miner);
                    let font = FontId::new(tokens::TEXT, fonts::strong());
                    let width = ctx.fonts_mut(|fonts| fonts.layout_no_wrap(name.clone(), font, egui::Color32::WHITE).size().x);
                    if width > NAME - 12.0 {
                        cut.push(format!("{name} {width:.0}"));
                    }
                    assert_eq!(name.matches('…').count(), usize::from(super::super::finder_address(&miner).is_some()), "{name}");
                }
            }
        }
        assert!(cut.is_empty(), "wider than {}: {cut:?}", NAME - 12.0);
    }
}
