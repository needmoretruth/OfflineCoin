//! The side bar's icons, drawn from the same strokes so they sit together.
use egui::{vec2, Color32, Painter, Pos2, Rect, Shape, Stroke};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Console,
    Network,
    Mining,
    Wallet,
    Boards,
    Nodes,
    Experiments,
    Settings,
    Keys,
    Lock,
    Explorer,
    Pool,
    Channel,
    Swap,
}

const WIDTH: f32 = 1.5;

pub fn paint(painter: &Painter, rect: Rect, icon: Icon, colour: Color32) {
    let stroke = Stroke::new(WIDTH, colour);
    let size = rect.width().min(rect.height());
    let origin = rect.center() - vec2(size, size) / 2.0;
    // Drawn on a grid of 18.
    let at = |x: f32, y: f32| origin + vec2(x, y) * (size / 18.0);
    let scale = size / 18.0;
    let line = |points: &[(f32, f32)]| {
        let points: Vec<Pos2> = points.iter().map(|(x, y)| at(*x, *y)).collect();
        painter.add(Shape::line(points, stroke));
    };
    let boxed = |left: f32, top: f32, right: f32, bottom: f32, corner: f32| {
        let area = Rect::from_min_max(at(left, top), at(right, bottom));
        painter.rect_stroke(area, corner, stroke, egui::StrokeKind::Middle);
    };
    match icon {
        Icon::Console => {
            boxed(2.5, 3.5, 15.5, 14.5, 2.0);
            line(&[(5.5, 7.0), (8.0, 9.0), (5.5, 11.0)]);
            line(&[(9.5, 11.0), (12.5, 11.0)]);
        }
        Icon::Network => {
            for (x, y) in [(9.0, 3.5), (3.5, 14.0), (14.5, 14.0)] {
                painter.circle_stroke(at(x, y), 2.2 * scale, stroke);
            }
            line(&[(7.9, 5.4), (4.6, 12.1)]);
            line(&[(10.1, 5.4), (13.4, 12.1)]);
            line(&[(5.7, 14.0), (12.3, 14.0)]);
        }
        Icon::Mining => {
            line(&[(3.0, 15.0), (10.5, 7.5)]);
            line(&[(5.0, 3.5), (9.5, 3.5), (14.5, 8.5), (14.5, 13.0)]);
            line(&[(8.5, 2.5), (15.5, 9.5)]);
        }
        Icon::Wallet => {
            boxed(2.5, 4.5, 15.5, 14.5, 2.0);
            boxed(11.0, 8.0, 15.5, 11.0, 1.0);
            line(&[(4.0, 4.5), (11.5, 2.5), (13.0, 4.5)]);
        }
        Icon::Boards => {
            boxed(4.5, 4.5, 13.5, 13.5, 1.5);
            boxed(7.5, 7.5, 10.5, 10.5, 0.5);
            for offset in [7.0, 11.0] {
                line(&[(offset, 2.0), (offset, 4.5)]);
                line(&[(offset, 13.5), (offset, 16.0)]);
                line(&[(2.0, offset), (4.5, offset)]);
                line(&[(13.5, offset), (16.0, offset)]);
            }
        }
        Icon::Nodes => {
            boxed(2.5, 3.0, 15.5, 7.5, 1.5);
            boxed(2.5, 10.5, 15.5, 15.0, 1.5);
            painter.circle_filled(at(5.5, 5.25), 0.9 * scale, colour);
            painter.circle_filled(at(5.5, 12.75), 0.9 * scale, colour);
        }
        Icon::Experiments => {
            line(&[(7.0, 2.5), (11.0, 2.5)]);
            line(&[(7.5, 2.5), (7.5, 7.0), (3.2, 14.0), (4.2, 15.5), (13.8, 15.5), (14.8, 14.0), (10.5, 7.0), (10.5, 2.5)]);
            line(&[(5.6, 11.0), (12.4, 11.0)]);
        }
        Icon::Settings => {
            for (y, knob) in [(4.5, 11.5), (9.0, 6.0), (13.5, 12.5)] {
                line(&[(2.5, y), (knob - 1.8, y)]);
                line(&[(knob + 1.8, y), (15.5, y)]);
                painter.circle_stroke(at(knob, y), 1.8 * scale, stroke);
            }
        }
        Icon::Keys => {
            painter.circle_stroke(at(5.5, 9.0), 3.0 * scale, stroke);
            line(&[(8.5, 9.0), (15.5, 9.0)]);
            line(&[(12.5, 9.0), (12.5, 12.0)]);
            line(&[(15.5, 9.0), (15.5, 12.0)]);
        }
        Icon::Lock => {
            boxed(4.0, 8.5, 14.0, 15.5, 1.5);
            line(&[(6.0, 8.5), (6.0, 5.5), (7.0, 3.5), (9.0, 2.8), (11.0, 3.5), (12.0, 5.5), (12.0, 8.5)]);
        }
        Icon::Channel => {
            painter.circle_stroke(at(3.8, 9.0), 2.2 * scale, stroke);
            painter.circle_stroke(at(14.2, 9.0), 2.2 * scale, stroke);
            line(&[(6.0, 7.6), (12.0, 7.6)]);
            line(&[(6.0, 10.4), (12.0, 10.4)]);
        }
        Icon::Explorer => {
            painter.circle_stroke(at(7.5, 7.5), 4.5 * scale, stroke);
            line(&[(11.0, 11.0), (15.5, 15.5)]);
        }
        Icon::Pool => {
            for (x, y) in [(3.5, 4.0), (9.0, 4.0), (14.5, 4.0)] {
                painter.circle_stroke(at(x, y), 1.8 * scale, stroke);
                line(&[(x, y + 1.8), (9.0, 11.5)]);
            }
            boxed(6.5, 11.5, 11.5, 15.5, 1.0);
        }
        Icon::Swap => {
            line(&[(3.0, 6.0), (15.0, 6.0)]);
            line(&[(12.0, 3.0), (15.0, 6.0), (12.0, 9.0)]);
            line(&[(15.0, 12.0), (3.0, 12.0)]);
            line(&[(6.0, 9.0), (3.0, 12.0), (6.0, 15.0)]);
        }
    }
}
