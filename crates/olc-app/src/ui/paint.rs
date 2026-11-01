//! The surfaces: light from above on the background, gradient faces, a thin bright edge, and a
//! soft shadow only on what floats.
use super::tokens::Tokens;
use egui::epaint::{Mesh, Shadow, Vertex, WHITE_UV};
use egui::{pos2, vec2, Color32, CornerRadius, Galley, Painter, Pos2, Rect, Shape, Stroke, StrokeKind};
use std::f32::consts::{FRAC_PI_2, PI};
use std::sync::Arc;

const CORNER_STEPS: usize = 6;
/// Enough squares that the light has no visible steps at the largest window.
const BACK_COLUMNS: u32 = 32;
const BACK_ROWS: u32 = 24;
/// Where the light on the background sits and how far it reaches, in parts of the window.
const LIGHT_AT: f32 = -0.1;
const LIGHT_WIDTH: f32 = 1.2;
const LIGHT_HEIGHT: f32 = 0.8;
const LIGHT_MIDDLE: f32 = 0.55;

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

/// The window's background: one light from above the top edge.
pub fn lit_back(rect: Rect, t: &Tokens) -> Shape {
    let shade = |x: f32, y: f32| {
        let across = (x - 0.5) / LIGHT_WIDTH;
        let down = (y - LIGHT_AT) / LIGHT_HEIGHT;
        let reach = (across * across + down * down).sqrt();
        if reach < LIGHT_MIDDLE {
            mix(t.back_glow, t.back_mid, reach / LIGHT_MIDDLE)
        } else {
            mix(t.back_mid, t.back_floor, ((reach - LIGHT_MIDDLE) / (1.0 - LIGHT_MIDDLE)).min(1.0))
        }
    };
    let mut mesh = Mesh::default();
    for row in 0..=BACK_ROWS {
        for column in 0..=BACK_COLUMNS {
            let (x, y) = (column as f32 / BACK_COLUMNS as f32, row as f32 / BACK_ROWS as f32);
            let pos = rect.min + vec2(x * rect.width(), y * rect.height());
            mesh.vertices.push(Vertex { pos, uv: WHITE_UV, color: shade(x, y) });
        }
    }
    let at = |column: u32, row: u32| row * (BACK_COLUMNS + 1) + column;
    for row in 0..BACK_ROWS {
        for column in 0..BACK_COLUMNS {
            mesh.add_triangle(at(column, row), at(column + 1, row), at(column, row + 1));
            mesh.add_triangle(at(column + 1, row), at(column + 1, row + 1), at(column, row + 1));
        }
    }
    Shape::mesh(mesh)
}

/// Text whose ink fades from `top` to `bottom`, for the largest figure on a screen.
pub fn ink(galley: &Galley, top: Color32, bottom: Color32) -> Arc<Galley> {
    let mut lit = galley.clone();
    let height = galley.size().y.max(1.0);
    for row in &mut lit.rows {
        let offset = row.pos.y;
        let placed = Arc::make_mut(&mut row.row);
        for vertex in &mut placed.visuals.mesh.vertices {
            vertex.color = mix(top, bottom, ((vertex.pos.y + offset) / height).clamp(0.0, 1.0));
        }
    }
    Arc::new(lit)
}

/// A thin line that fades out at both ends.
pub fn fading_line(painter: &Painter, left: f32, right: f32, y: f32, colour: Color32) {
    let fade = ((right - left) * 0.06).min(48.0);
    let clear = Color32::TRANSPARENT;
    let stops = [(left, clear), (left + fade, colour), (right - fade, colour), (right, clear)];
    let mut mesh = Mesh::default();
    for (x, color) in stops {
        mesh.vertices.push(Vertex { pos: pos2(x, y - 0.5), uv: WHITE_UV, color });
        mesh.vertices.push(Vertex { pos: pos2(x, y + 0.5), uv: WHITE_UV, color });
    }
    for step in 0..3 {
        mesh.add_triangle(step * 2, step * 2 + 1, step * 2 + 2);
        mesh.add_triangle(step * 2 + 1, step * 2 + 3, step * 2 + 2);
    }
    painter.add(Shape::mesh(mesh));
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
