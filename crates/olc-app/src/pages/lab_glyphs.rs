//! A shape for each kind of participant, so a person tells them apart without reading: a miner is
//! a pick, a pool a ring, the three keepers a stack of blocks (whole, cut, outlined), money moves
//! as an arrow out or in, spam as many dots, and the two who cheat are a diamond and a warning.
//! Each is drawn in `size` units around a centre, in one colour.
use crate::local::lab_net::Kind;
use crate::ui::tokens::Tokens;
use egui::{vec2, Color32, CornerRadius, Painter, Pos2, Rect, Shape, Stroke, StrokeKind};

/// The colour of a kind's token: grey steps for honest kinds, red for the two who cheat.
pub fn fill(t: &Tokens, kind: Kind) -> Color32 {
    let grey = |step: u8| if t.dark { Color32::from_gray(52 + step * 14) } else { Color32::from_gray(222 - step * 16) };
    match kind {
        Kind::Attacker | Kind::Exploiter => t.bad.gamma_multiply(0.85),
        Kind::Miner => grey(3),
        Kind::Pool => grey(2),
        Kind::Full | Kind::Pruned | Kind::Light => grey(1),
        Kind::Sender | Kind::Receiver | Kind::Spammer => grey(0),
    }
}

/// The colour drawn on a token's fill.
pub fn ink(t: &Tokens, kind: Kind) -> Color32 {
    match kind {
        Kind::Attacker | Kind::Exploiter => Color32::WHITE,
        _ if t.dark => Color32::from_gray(238),
        _ => Color32::from_gray(30),
    }
}

/// Draws the shape of `kind` around `centre`, `size` across.
pub fn paint(painter: &Painter, kind: Kind, centre: Pos2, size: f32, colour: Color32) {
    let unit = size / 18.0;
    let at = |x: f32, y: f32| centre + vec2(x * unit, y * unit);
    let line = Stroke::new((1.8 * unit).max(1.2), colour);
    let block = |x: f32, y: f32, w: f32, h: f32| Rect::from_min_size(at(x, y), vec2(w * unit, h * unit));
    let round = CornerRadius::same((unit * 1.5) as u8);
    match kind {
        Kind::Miner => {
            // A pick: a curved head on a handle.
            painter.line_segment([at(-6.0, 7.0), at(5.0, -4.0)], line);
            let head = [at(-3.0, -8.0), at(2.0, -7.5), at(6.0, -5.0), at(8.0, -1.0), at(8.5, 3.0)];
            painter.add(Shape::line(head.to_vec(), line));
        }
        Kind::Pool => {
            painter.circle_stroke(centre, 7.5 * unit, line);
            for angle in [0.0_f32, 2.094, 4.189] {
                painter.circle_filled(centre + vec2(angle.cos(), angle.sin()) * 3.5 * unit, 1.8 * unit, colour);
            }
        }
        Kind::Full | Kind::Pruned | Kind::Light => {
            for (row, y) in [-7.0, -1.5, 4.0].into_iter().enumerate() {
                let rect = block(-7.0, y, 14.0, 4.0);
                let hollow = kind == Kind::Light || (kind == Kind::Pruned && row == 2);
                if hollow {
                    painter.rect_stroke(rect, round, Stroke::new(unit * 1.2, colour), StrokeKind::Inside);
                } else {
                    painter.rect_filled(rect, round, colour);
                }
            }
        }
        Kind::Sender | Kind::Receiver => {
            painter.circle_stroke(at(-2.5, 0.0), 5.5 * unit, line);
            let (from, to) = if kind == Kind::Sender { (at(1.0, 0.0), at(8.5, 0.0)) } else { (at(8.5, 0.0), at(1.0, 0.0)) };
            painter.line_segment([from, to], line);
            let back = if kind == Kind::Sender { -3.0 } else { 3.0 };
            painter.line_segment([to, to + vec2(back * unit, -3.0 * unit)], line);
            painter.line_segment([to, to + vec2(back * unit, 3.0 * unit)], line);
        }
        Kind::Spammer => {
            for (x, y) in [(-6.0, -5.0), (0.0, -6.0), (6.0, -4.0), (-4.0, 1.0), (2.5, 0.5), (-1.0, 6.0), (5.5, 6.0), (-7.0, 6.5)] {
                painter.circle_filled(at(x, y), 1.9 * unit, colour);
            }
        }
        Kind::Attacker => {
            let diamond = vec![at(0.0, -8.5), at(8.0, 0.0), at(0.0, 8.5), at(-8.0, 0.0)];
            painter.add(Shape::closed_line(diamond, line));
            painter.circle_filled(centre, 2.6 * unit, colour);
        }
        Kind::Exploiter => {
            let triangle = vec![at(0.0, -8.5), at(8.5, 7.0), at(-8.5, 7.0)];
            painter.add(Shape::closed_line(triangle, line));
            painter.line_segment([at(0.0, -3.0), at(0.0, 2.0)], line);
            painter.circle_filled(at(0.0, 4.6), 1.1 * unit, colour);
        }
    }
}

/// A pause mark, drawn over a token that does not run.
pub fn paused(painter: &Painter, centre: Pos2, size: f32, colour: Color32) {
    let unit = size / 18.0;
    for x in [-3.0, 2.0] {
        let rect = Rect::from_min_size(centre + vec2(x * unit, -5.0 * unit), vec2(2.4 * unit, 10.0 * unit));
        painter.rect_filled(rect, CornerRadius::same(1), colour);
    }
}

/// A check, for a payment that settled.
pub fn check(painter: &Painter, centre: Pos2, size: f32, colour: Color32) {
    let unit = size / 18.0;
    let points = vec![centre + vec2(-6.0, 0.5) * unit, centre + vec2(-2.0, 4.5) * unit, centre + vec2(6.5, -4.5) * unit];
    painter.add(Shape::line(points, Stroke::new(2.2 * unit, colour)));
}

/// A cross, for a refusal or a payment undone.
pub fn cross(painter: &Painter, centre: Pos2, size: f32, colour: Color32) {
    let unit = size / 18.0;
    let stroke = Stroke::new(2.2 * unit, colour);
    painter.line_segment([centre + vec2(-5.0, -5.0) * unit, centre + vec2(5.0, 5.0) * unit], stroke);
    painter.line_segment([centre + vec2(5.0, -5.0) * unit, centre + vec2(-5.0, 5.0) * unit], stroke);
}
