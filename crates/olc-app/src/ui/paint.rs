//! The surfaces: lit from above, with a soft shadow and a thin bright edge.
use egui::epaint::{Mesh, Shadow, Vertex, WHITE_UV};
use egui::{pos2, Color32, CornerRadius, Painter, Pos2, Rect, Shape, Stroke, StrokeKind};
use std::f32::consts::{FRAC_PI_2, PI};

const CORNER_STEPS: usize = 6;

fn mix(top: Color32, bottom: Color32, part: f32) -> Color32 {
    let channel = |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * part) as u8;
    Color32::from_rgba_premultiplied(
        channel(top.r(), bottom.r()),
        channel(top.g(), bottom.g()),
        channel(top.b(), bottom.b()),
        channel(top.a(), bottom.a()),
    )
}

fn outline(rect: Rect, radius: f32) -> Vec<Pos2> {
    let corners = [
        (rect.right_top() + egui::vec2(-radius, radius), -FRAC_PI_2),
        (rect.right_bottom() + egui::vec2(-radius, -radius), 0.0),
        (rect.left_bottom() + egui::vec2(radius, -radius), FRAC_PI_2),
        (rect.left_top() + egui::vec2(radius, radius), PI),
    ];
    let mut points = Vec::with_capacity(4 * (CORNER_STEPS + 1));
    for (center, start) in corners {
        for step in 0..=CORNER_STEPS {
            let angle = start + FRAC_PI_2 * step as f32 / CORNER_STEPS as f32;
            points.push(center + radius * egui::vec2(angle.cos(), angle.sin()));
        }
    }
    points
}

/// A vertical gradient inside rounded corners.
pub fn gradient(rect: Rect, radius: f32, top: Color32, bottom: Color32) -> Shape {
    let colour = |y: f32| mix(top, bottom, ((y - rect.top()) / rect.height()).clamp(0.0, 1.0));
    let mut mesh = Mesh::default();
    let vertex = |pos: Pos2| Vertex {
        pos,
        uv: WHITE_UV,
        color: colour(pos.y),
    };
    mesh.vertices.push(vertex(rect.center()));
    let points = outline(rect, radius);
    let count = points.len() as u32;
    mesh.vertices.extend(points.into_iter().map(vertex));
    for index in 0..count {
        mesh.add_triangle(0, index + 1, (index + 1) % count + 1);
    }
    Shape::mesh(mesh)
}

pub struct Surface {
    pub top: Color32,
    pub bottom: Color32,
    pub line: Color32,
    pub glint: Color32,
    pub shadow: Option<Shadow>,
    pub radius: u8,
}

/// Paints into `slot`, a place taken before the content was drawn on top of it.
pub fn surface(painter: &Painter, rect: Rect, look: &Surface, slot: Option<egui::layers::ShapeIdx>) {
    let round = CornerRadius::same(look.radius);
    let mut shapes = Vec::with_capacity(4);
    if let Some(shadow) = look.shadow {
        shapes.push(Shape::from(shadow.as_shape(rect, round)));
    }
    shapes.push(gradient(rect, f32::from(look.radius), look.top, look.bottom));
    shapes.push(Shape::rect_stroke(
        rect,
        round,
        Stroke::new(1.0, look.line),
        StrokeKind::Inside,
    ));
    let inset = f32::from(look.radius);
    if look.glint.a() > 0 && rect.width() > inset * 2.0 {
        let y = rect.top() + 1.5;
        shapes.push(Shape::line_segment(
            [pos2(rect.left() + inset, y), pos2(rect.right() - inset, y)],
            Stroke::new(1.0, look.glint),
        ));
    }
    let shape = Shape::Vec(shapes);
    match slot {
        Some(slot) => painter.set(slot, shape),
        None => {
            painter.add(shape);
        }
    }
}
