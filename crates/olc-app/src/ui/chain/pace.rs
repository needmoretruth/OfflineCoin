//! How fast this computer mines and how far the next block has come.
use super::{wait, Look};
use crate::core::ChainView;
use crate::i18n;
use crate::ui::{fonts, format, paint, tokens};
use egui::epaint::{Mesh, Vertex, WHITE_UV};
use egui::{pos2, vec2, Align2, Color32, FontFamily, FontId, Pos2, Rect, Response, Sense, Shape, Stroke, Ui};
use std::time::Duration;

/// The line covers this long, ending now.
const SPAN_MS: u64 = 600_000;
const TRACK: f32 = 8.0;
/// How often the bar moves while a block is awaited.
const BAR_STEP: Duration = Duration::from_millis(100);

/// This computer's speed over the last ten minutes: the number now above a thin line on a soft
/// fill, `height` points tall in all.
pub fn hashrate_line(ui: &mut Ui, look: &Look, chain: &ChainView, height: f32) -> Response {
    let t = look.t;
    let voice = look.voice;
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(vec2(width, height.max(72.0)), Sense::hover());
    let painter = ui.painter_at(rect);
    let small = FontId::new(tokens::META, FontFamily::Proportional);
    painter.text(rect.left_top(), Align2::LEFT_TOP, i18n::text(voice, "chain.speed"), small.clone(), t.soft);
    let now = format::rate(voice, chain.hashrate_here);
    let big = FontId::new(tokens::TITLE, fonts::strong());
    let number_at = pos2(rect.left(), rect.top() + tokens::META_LINE + 2.0);
    let number = painter.text(number_at, Align2::LEFT_TOP, &now, big, t.text);
    let plot = Rect::from_min_max(pos2(rect.left(), number.bottom() + 10.0), pos2(rect.right(), rect.bottom() - tokens::META_LINE));
    let accent = t.info;
    painter.line_segment([plot.left_bottom(), plot.right_bottom()], Stroke::new(1.0, t.line_strong));
    let ends = [(Align2::LEFT_TOP, plot.left_bottom(), "chain.ten_minutes"), (Align2::RIGHT_TOP, plot.right_bottom(), "chain.now")];
    for (anchor, at, key) in ends {
        painter.text(at + vec2(0.0, 4.0), anchor, i18n::text(voice, key), small.clone(), t.muted);
    }
    let most = chain.trail.iter().map(|(_, rate)| *rate).max().unwrap_or(0);
    if most == 0 || chain.trail.is_empty() {
        let said = i18n::text(voice, "chain.not_mining");
        painter.text(plot.center(), Align2::CENTER_CENTER, said, small, t.muted);
        return labelled(response, &format!("{}, {said}", i18n::text(voice, "chain.speed")));
    }
    let top = most as f32 * 1.15;
    let end_ms = look.now_ms.max(chain.trail.last().map_or(0, |(at, _)| *at));
    let point = |(at, rate): (u64, u64)| {
        let back = end_ms.saturating_sub(at).min(SPAN_MS) as f32 / SPAN_MS as f32;
        pos2(plot.right() - back * plot.width(), plot.bottom() - rate as f32 / top * plot.height())
    };
    let mut points: Vec<Pos2> = chain.trail.iter().copied().map(point).collect();
    if let Some(last) = points.last().copied() {
        // The speed holds until the next reading, so the line runs on to now.
        points.push(pos2(plot.right(), last.y));
    }
    painter.add(soft_fill(&points, plot.bottom(), accent));
    painter.add(Shape::line(points, Stroke::new(1.6, accent)));
    painter.text(plot.left_top(), Align2::LEFT_TOP, format::rate(voice, most), small, t.muted);
    labelled(response, &format!("{}, {now}", i18n::text(voice, "chain.speed")))
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
pub fn next_block_bar(ui: &mut Ui, look: &Look, chain: &ChainView) -> Response {
    let t = look.t;
    let voice = look.voice;
    let width = ui.available_width();
    let height = tokens::META_LINE * 2.0 + TRACK + 14.0;
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    let painter = ui.painter_at(rect);
    let text_font = FontId::new(tokens::TEXT, fonts::medium());
    let small = FontId::new(tokens::META, FontFamily::Proportional);
    painter.text(rect.left_top(), Align2::LEFT_TOP, i18n::text(voice, "chain.next"), text_font, t.text);
    let track = Rect::from_min_size(pos2(rect.left(), rect.top() + tokens::META_LINE + 6.0), vec2(width, TRACK));
    painter.rect_filled(track, TRACK / 2.0, t.hover);
    let (Some(expected), Some(last)) = (chain.expected_ms, chain.last_block_ms()) else {
        let said = i18n::text(voice, if chain.latest.is_empty() { "chain.no_blocks" } else { "chain.nobody" });
        painter.text(rect.right_top(), Align2::RIGHT_TOP, said, small, t.muted);
        return labelled(response, said);
    };
    let since = look.now_ms.saturating_sub(last);
    let part = (since as f32 / expected.max(1) as f32).min(1.0);
    let fill = Rect::from_min_size(track.min, vec2((track.width() * part).max(TRACK), TRACK));
    let colours = super::faces(t, false);
    painter.add(paint::gradient(fill, TRACK / 2.0, colours.top, colours.front_bottom));
    let expected_text = i18n::fill(i18n::text(voice, "chain.expected"), &[&wait(voice, expected)]);
    painter.text(rect.right_top(), Align2::RIGHT_TOP, &expected_text, small.clone(), t.soft);
    let below = track.left_bottom() + vec2(0.0, 6.0);
    let since_text = i18n::fill(i18n::text(voice, "chain.since_last"), &[&wait(voice, since)]);
    let left = if since > expected.saturating_mul(2) { format!("{since_text}. {}", i18n::text(voice, "chain.overdue")) } else { since_text };
    painter.text(below, Align2::LEFT_TOP, &left, small.clone(), t.soft);
    let average = chain.average_ms.map(|ms| i18n::fill(i18n::text(voice, "chain.average"), &[&wait(voice, ms)]));
    if let Some(average) = &average {
        painter.text(pos2(track.right(), below.y), Align2::RIGHT_TOP, average, small, t.muted);
    }
    if look.still {
        ui.ctx().request_repaint_after(Duration::from_secs(1));
    } else {
        ui.ctx().request_repaint_after(BAR_STEP);
    }
    let label = [Some(i18n::text(voice, "chain.next").to_string()), Some(expected_text), Some(left), average];
    labelled(response, &label.into_iter().flatten().collect::<Vec<_>>().join(", "))
}
