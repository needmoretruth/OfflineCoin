//! How fast this computer mines and how far the next block has come.
use super::{wait, Look};
use crate::core::ChainView;
use crate::i18n;
use crate::ui::{fonts, format, paint, tokens};
use egui::epaint::{Mesh, Vertex, WHITE_UV};
use egui::{pos2, vec2, Align2, Color32, FontFamily, FontId, Pos2, Rect, Response, Sense, Shape, Stroke, Ui};
use std::time::Duration;

/// The line covers at most this long, ending now.
const SPAN_MS: u64 = 600_000;
/// Less data than this makes no line worth reading.
const MINUTE_MS: u64 = 60_000;
const TRACK: f32 = 8.0;
/// How often the bar moves while a block is awaited.
const BAR_STEP: Duration = Duration::from_millis(100);

/// The most every miner on this computer may hash together: the person's limit, or the highest
/// speed on the mining threads without one. The speed line draws it as its guide, so a new limit
/// or thread cap moves the guide at once instead of after ten minutes of readings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ceiling {
    pub rate: u64,
    pub limited: bool,
}

impl Ceiling {
    pub fn of(budget: &crate::local::Budget) -> Self {
        let limited = budget.limit > 0 && (budget.highest == 0 || budget.limit < budget.highest);
        Self { rate: budget.total, limited }
    }
}

/// Every miner on this computer together, over up to the last ten minutes: the name and the number
/// now on one line, a thin line on a soft fill under them, `height` points tall in all. The line
/// spans as long as there is data for, so the first minutes fill the width rather than a sliver;
/// under a minute it says when the line starts. The ceiling is a labelled guide.
pub fn hashrate_line(ui: &mut Ui, look: &Look, chain: &ChainView, ceiling: &Ceiling, height: f32) -> Response {
    let t = look.t;
    let voice = look.voice;
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(vec2(width, height.max(64.0)), Sense::hover());
    let painter = ui.painter_at(rect);
    let small = FontId::new(tokens::META, FontFamily::Proportional);
    let title = i18n::text(voice, "chain.speed");
    painter.text(rect.left_top(), Align2::LEFT_TOP, title, small.clone(), t.soft);
    let now = format::rate(voice, chain.hashrate_here);
    let figure = FontId::new(tokens::TEXT, fonts::strong());
    painter.text(rect.right_top(), Align2::RIGHT_TOP, &now, figure, t.text);
    let plot = Rect::from_min_max(pos2(rect.left(), rect.top() + tokens::META_LINE + 10.0), pos2(rect.right(), rect.bottom() - tokens::META_LINE));
    painter.line_segment([plot.left_bottom(), plot.right_bottom()], Stroke::new(1.0, t.line_strong));
    let most = chain.trail.iter().map(|(_, rate)| *rate).max().unwrap_or(0);
    let end_ms = look.now_ms.max(chain.trail.last().map_or(0, |(at, _)| *at));
    let first_ms = chain.trail.first().map_or(end_ms, |(at, _)| *at);
    let gathered = end_ms.saturating_sub(first_ms);
    let quiet = match () {
        () if most == 0 => Some("chain.not_mining"),
        () if gathered < MINUTE_MS => Some("chain.line_soon"),
        () => None,
    };
    if let Some(key) = quiet {
        let said = i18n::text(voice, key);
        painter.text(plot.center(), Align2::CENTER_CENTER, said, small, t.muted);
        return labelled(response, &format!("{title}, {now}, {said}"));
    }
    // The line fills the width with what there is; the far end says it to the nearest minute.
    let span = gathered.min(SPAN_MS);
    let minutes = (span + MINUTE_MS / 2) / MINUTE_MS;
    let far = i18n::fill(i18n::text(voice, "chain.minutes_ago"), &[&minutes.to_string()]);
    painter.text(plot.left_bottom() + vec2(0.0, 4.0), Align2::LEFT_TOP, &far, small.clone(), t.muted);
    painter.text(plot.right_bottom() + vec2(0.0, 4.0), Align2::RIGHT_TOP, i18n::text(voice, "chain.now"), small.clone(), t.muted);
    // The ceiling is the guide; a reading above it, from before a limit was lowered, still fits.
    let guide = if ceiling.rate > 0 { ceiling.rate } else { most };
    let top = guide.max(most) as f32 * 1.25;
    let point = |(at, rate): (u64, u64)| {
        let back = end_ms.saturating_sub(at).min(span) as f32 / span as f32;
        pos2(plot.right() - back * plot.width(), plot.bottom() - rate as f32 / top * plot.height())
    };
    let peak_y = plot.bottom() - guide as f32 / top * plot.height();
    painter.line_segment([pos2(plot.left(), peak_y), pos2(plot.right(), peak_y)], Stroke::new(1.0, t.line));
    let key = match (ceiling.rate, ceiling.limited) {
        (0, _) => "chain.peak",
        (_, true) => "chain.ceiling_limit",
        (_, false) => "chain.ceiling_full",
    };
    let peak = i18n::fill(i18n::text(voice, key), &[&format::rate(voice, guide)]);
    painter.text(pos2(plot.left(), peak_y - 2.0), Align2::LEFT_BOTTOM, &peak, small, t.muted);
    let accent = t.info;
    let mut points: Vec<Pos2> = chain.trail.iter().copied().map(point).collect();
    if let Some(last) = points.last().copied() {
        // The speed holds until the next reading, so the line runs on to now.
        points.push(pos2(plot.right(), last.y));
    }
    painter.add(soft_fill(&points, plot.bottom(), accent));
    painter.add(Shape::line(points, Stroke::new(1.6, accent)));
    labelled(response, &format!("{title}, {now}, {peak}"))
}

fn labelled(response: Response, label: &str) -> Response {
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, label));
    response
}

/// The area under the line, strongest at the line and gone at the floor.
fn soft_fill(points: &[Pos2], floor: f32, colour: Color32) -> Shape {
    let mut mesh = Mesh::default();
    let at_line = colour.gamma_multiply(0.22);
    for point in points {
        mesh.vertices.push(Vertex { pos: *point, uv: WHITE_UV, color: at_line });
        mesh.vertices.push(Vertex { pos: pos2(point.x, floor), uv: WHITE_UV, color: Color32::TRANSPARENT });
    }
    for step in 0..points.len().saturating_sub(1) as u32 {
        let at = step * 2;
        mesh.add_triangle(at, at + 1, at + 2);
        mesh.add_triangle(at + 1, at + 3, at + 2);
    }
    Shape::mesh(mesh)
}

/// The time since the last block against the expected wait, with the real average of the last
/// blocks beside it. Past the expected wait the bar stays full and says that blocks come by luck.
/// When the two lines under the bar do not fit side by side, the average takes a line of its own.
pub fn next_block_bar(ui: &mut Ui, look: &Look, chain: &ChainView) -> Response {
    let t = look.t;
    let voice = look.voice;
    let width = ui.available_width();
    let text_font = FontId::new(tokens::TEXT, fonts::medium());
    let small = FontId::new(tokens::META, FontFamily::Proportional);
    let known = chain.expected_ms.zip(chain.last_block_ms());
    let lines = known.map(|(expected, last)| {
        let since = look.now_ms.saturating_sub(last);
        let since_text = i18n::fill(i18n::text(voice, "chain.since_last"), &[&wait(voice, since)]);
        let left = if since > expected.saturating_mul(2) { format!("{since_text}. {}", i18n::text(voice, "chain.overdue")) } else { since_text };
        let average = chain.average_ms.map(|ms| i18n::fill(i18n::text(voice, "chain.average"), &[&wait(voice, ms)]));
        (expected, since, left, average)
    });
    let measure = |text: &str| ui.painter().layout_no_wrap(text.to_string(), small.clone(), t.soft).size().x;
    let stacked = lines.as_ref().is_some_and(|(_, _, left, average)| {
        average.as_ref().is_some_and(|average| measure(left) + measure(average) + tokens::GAP_WIDE > width)
    });
    let extra = if stacked { tokens::META_LINE } else { 0.0 };
    let height = tokens::META_LINE * 2.0 + TRACK + 14.0 + extra;
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.text(rect.left_top(), Align2::LEFT_TOP, i18n::text(voice, "chain.next"), text_font, t.text);
    let track = Rect::from_min_size(pos2(rect.left(), rect.top() + tokens::META_LINE + 6.0), vec2(width, TRACK));
    painter.rect_filled(track, TRACK / 2.0, t.hover);
    let Some((expected, since, left, average)) = lines else {
        let said = i18n::text(voice, if chain.latest.is_empty() { "chain.no_blocks" } else { "chain.nobody" });
        painter.text(rect.right_top(), Align2::RIGHT_TOP, said, small, t.muted);
        return labelled(response, said);
    };
    let part = (since as f32 / expected.max(1) as f32).min(1.0);
    let fill = Rect::from_min_size(track.min, vec2((track.width() * part).max(TRACK), TRACK));
    let colours = super::faces(t, false);
    let (start, end) = colours.bar(t);
    painter.add(paint::gradient(fill, TRACK / 2.0, start, end));
    let expected_text = i18n::fill(i18n::text(voice, "chain.expected"), &[&wait(voice, expected)]);
    painter.text(rect.right_top(), Align2::RIGHT_TOP, &expected_text, small.clone(), t.soft);
    let below = track.left_bottom() + vec2(0.0, 6.0);
    painter.text(below, Align2::LEFT_TOP, &left, small.clone(), t.soft);
    if let Some(average) = &average {
        let (at, anchor) = if stacked {
            (below + vec2(0.0, tokens::META_LINE), Align2::LEFT_TOP)
        } else {
            (pos2(track.right(), below.y), Align2::RIGHT_TOP)
        };
        painter.text(at, anchor, average, small, t.muted);
    }
    if look.still {
        ui.ctx().request_repaint_after(Duration::from_secs(1));
    } else {
        ui.ctx().request_repaint_after(BAR_STEP);
    }
    let label = [Some(i18n::text(voice, "chain.next").to_string()), Some(expected_text), Some(left), average];
    labelled(response, &label.into_iter().flatten().collect::<Vec<_>>().join(", "))
}
