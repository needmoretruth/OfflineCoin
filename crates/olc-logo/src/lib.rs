//! The logo: a real 3D coin, stamped with the Wi-Fi symbol crossed out. One renderer serves the
//! terminal, the app and the OLED, each of which only decides how to show the brightness.
#![no_std]

extern crate alloc;

mod reel;

pub use reel::Reel;

use libm::{atan2f, cosf, sinf, sqrtf};

const THICKNESS: f32 = 0.17;
const RIM: f32 = 0.87;
const REEDS: f32 = 36.0;
/// The coin stays inside the frame at every angle.
const FIT: f32 = 1.2;
/// How far the eye sits from the coin, in coin radii. Closer shows more perspective.
const EYE: f32 = 3.2;
/// How far the turning axis leans towards the viewer, in radians.
const LEAN: f32 = 0.30;
const SHEEN_TRAVEL: f32 = 1.6;
const SHEEN_WIDTH: f32 = 0.32;
const SHEEN_SLANT: f32 = 0.4;

/// Whether a point on the coin's face belongs to the stamped symbol. The face runs from -1 to 1.
fn stamped(x: f32, y: f32) -> bool {
    let (px, py) = (x, y + 0.52);
    let distance = sqrtf(px * px + py * py);
    let in_fan = px.abs() <= py * 1.05;
    let arc = |inner: f32, outer: f32| in_fan && distance >= inner && distance <= outer;
    let symbol = distance < 0.13 || arc(0.33, 0.47) || arc(0.65, 0.79) || arc(0.97, 1.10);
    // The stroke runs from upper left to lower right, along y = -x.
    let across = (x + y).abs() / core::f32::consts::SQRT_2;
    let along = (x - y).abs() / core::f32::consts::SQRT_2;
    let stroke = across < 0.075 && along < 0.86;
    let gap = across < 0.17;
    stroke || (symbol && !gap)
}

struct Hit {
    /// How much light this part of the metal gives back.
    albedo: f32,
}

fn face(x: f32, y: f32, z_sign: f32) -> Hit {
    // The back shows the same symbol, the right way round.
    let u = x * z_sign;
    let radius = sqrtf(x * x + y * y);
    let albedo = if radius > RIM || stamped(u, y) {
        1.0
    } else {
        0.42
    };
    Hit { albedo }
}

fn edge(x: f32, y: f32) -> Hit {
    let reed = sinf(atan2f(y, x) * REEDS);
    Hit {
        albedo: 0.62 + 0.38 * reed,
    }
}

/// One turn on every surface, so the board, the terminal and the app spin alike.
pub const TURN_MS: u64 = 9_000;
/// Frames in a turn. No frame shows twice at [`MAX_FPS`].
pub const FRAMES: usize = (TURN_MS * MAX_FPS as u64 / 1_000) as usize;
/// The most frames a second any surface shows.
pub const MAX_FPS: u32 = 30;
/// The fewest frames a second. Any slower and the steps grow towards a half turn. Both faces look
/// alike, so at that point the eye sees the coin swing to and fro instead of turning.
pub const MIN_FPS: u32 = 5;

const TAU: f32 = core::f32::consts::TAU;

/// The angle of a frame of the turn.
pub fn frame_angle(frame: usize) -> f32 {
    frame as f32 / FRAMES as f32 * TAU
}

/// The frame of the turn to show at a moment.
pub fn frame_at(now_ms: u64) -> usize {
    ((now_ms % TURN_MS) * FRAMES as u64 / TURN_MS) as usize
}

type Vector = [f32; 3];

fn dot(a: Vector, b: Vector) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// `a` plus `b` times `scale`.
fn along(a: Vector, b: Vector, scale: f32) -> Vector {
    [
        a[0] + b[0] * scale,
        a[1] + b[1] * scale,
        a[2] + b[2] * scale,
    ]
}

/// One rotation of the coin, kept so a frame costs a sine and a cosine once. The turn is seen in
/// true perspective from a little above, which is what tells the eye which way it goes: the near
/// side passes lower and faster than the far side, always to the right.
#[derive(Clone, Copy)]
pub struct Spin {
    /// The coin's own axes as seen by the viewer. Its face lies along the first two.
    axes: [Vector; 3],
    /// The eye in the coin's space.
    eye: Vector,
    /// Where the sheen stands on the frame.
    sheen: f32,
}

impl Spin {
    pub fn new(angle: f32) -> Self {
        let (sin, cos) = (sinf(angle), cosf(angle));
        let (lean_sin, lean_cos) = (sinf(LEAN), cosf(LEAN));
        let lean =
            |[x, y, z]: Vector| [x, y * lean_cos - z * lean_sin, y * lean_sin + z * lean_cos];
        let axes = [
            lean([cos, 0.0, -sin]),
            lean([0.0, 1.0, 0.0]),
            lean([sin, 0.0, cos]),
        ];
        let eye = [axes[0][2] * EYE, axes[1][2] * EYE, axes[2][2] * EYE];
        // Crosses the frame once for each face that passes, always the way the near side moves.
        let sheen = (if cos < 0.0 { -sin } else { sin }) * SHEEN_TRAVEL;
        Self { axes, eye, sheen }
    }

    /// Brightness from 0 to 1 at a point of a frame running from -1 to 1 with y up, or `None`
    /// off the coin.
    pub fn shade(self, x: f32, y: f32) -> Option<f32> {
        let (x, y) = (x * FIT, y * FIT);
        let [ex, ey, ez] = self.axes;
        let ray = [x, y, -EYE];
        let ray = [dot(ray, ex), dot(ray, ey), dot(ray, ez)];
        let eye = self.eye;

        // The ray is inside the round wall between these two moments.
        let round = ray[0] * ray[0] + ray[1] * ray[1];
        let near = eye[0] * ray[0] + eye[1] * ray[1];
        let room = near * near - round * (eye[0] * eye[0] + eye[1] * eye[1] - 1.0);
        if room < 0.0 || round < 1e-9 {
            return None;
        }
        let root = sqrtf(room);
        let (wall_in, wall_out) = ((-near - root) / round, (-near + root) / round);
        // And between the two faces between these.
        let (face_in, face_out) = if ray[2].abs() < 1e-6 {
            if eye[2].abs() > THICKNESS {
                return None;
            }
            (f32::MIN, f32::MAX)
        } else {
            let (a, b) = (
                (THICKNESS - eye[2]) / ray[2],
                (-THICKNESS - eye[2]) / ray[2],
            );
            (a.min(b), a.max(b))
        };
        if wall_in.max(face_in) > wall_out.min(face_out) {
            return None;
        }
        let (hit, normal) = if face_in > wall_in {
            let side = if eye[2] > 0.0 { 1.0 } else { -1.0 };
            let at = along(eye, ray, face_in);
            (face(at[0], at[1], side), along([0.0; 3], ez, side))
        } else {
            let at = along(eye, ray, wall_in);
            let normal = along(along([0.0; 3], ex, at[0]), ey, at[1]);
            (edge(at[0], at[1]), normal)
        };
        Some(self.light(normal, hit, x, y))
    }

    fn light(self, normal: Vector, hit: Hit, x: f32, y: f32) -> f32 {
        // One light from above and in front: nothing swaps sides as the coin turns.
        const KEY: Vector = [0.0, 0.30, 0.954];
        let diffuse = dot(normal, KEY).max(0.0);
        let band = (x + y * SHEEN_SLANT - self.sheen) / SHEEN_WIDTH;
        let sheen = (1.0 - band * band).max(0.0);
        ((0.30 + 0.62 * diffuse) * hit.albedo + 0.60 * sheen * sheen * hit.albedo).clamp(0.0, 1.0)
    }
}

/// See [`Spin::shade`].
pub fn shade(angle: f32, x: f32, y: f32) -> Option<f32> {
    Spin::new(angle).shade(x, y)
}

/// Whether a pixel lights on a one-bit panel. Bright metal is solid, the rest is dithered.
pub fn lit(spin: Spin, x: i32, y: i32, radius: i32) -> bool {
    const BAYER: [[f32; 4]; 4] = [
        [0.03, 0.53, 0.16, 0.66],
        [0.78, 0.28, 0.91, 0.41],
        [0.22, 0.72, 0.09, 0.59],
        [0.97, 0.47, 0.84, 0.34],
    ];
    const SOLID: f32 = 0.48;
    const DIM: f32 = 0.45;
    let (u, v) = (
        (x as f32 + 0.5) / radius as f32,
        -(y as f32 + 0.5) / radius as f32,
    );
    spin.shade(u, v).is_some_and(|light| {
        light > SOLID || light * DIM > BAYER[(y & 3) as usize][(x & 3) as usize]
    })
}

/// Darkest to brightest, for terminals.
const RAMP: &[u8] = b" .:-=+*#%@";

/// Fills one row of a text frame. A terminal cell is about twice as tall as it is wide, so a
/// frame of `columns` by `rows` looks round when `columns` is twice `rows`.
pub fn ascii_row(angle: f32, row: usize, rows: usize, columns: usize, mut put: impl FnMut(char)) {
    let spin = Spin::new(angle);
    let y = 1.0 - (row as f32 + 0.5) * 2.0 / rows as f32;
    for column in 0..columns {
        let x = (column as f32 + 0.5) * 2.0 / columns as f32 - 1.0;
        put(match spin.shade(x, y) {
            Some(light) => {
                let index = (light * (RAMP.len() - 1) as f32) as usize + 1;
                RAMP[index.min(RAMP.len() - 1)] as char
            }
            None => ' ',
        });
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::vec::Vec;

    const GRID: usize = 120;

    fn field(frame: usize) -> Vec<Option<f32>> {
        let spin = Spin::new(frame_angle(frame));
        let place = |index: usize| (index as f32 + 0.5) * 2.0 / GRID as f32 - 1.0;
        (0..GRID * GRID)
            .map(|at| spin.shade(place(at % GRID), -place(at / GRID)))
            .collect()
    }

    /// How far the coin's face moved to the right between two frames, by optical flow. Only the
    /// inside counts: the outline of any turning disc squeezes in from both sides.
    fn net_motion(from: &[Option<f32>], to: &[Option<f32>]) -> f32 {
        motion_within(from, to, 0..GRID)
    }

    fn motion_within(
        from: &[Option<f32>],
        to: &[Option<f32>],
        columns: core::ops::Range<usize>,
    ) -> f32 {
        const MARGIN: usize = 6;
        let inside = |at: usize| {
            (0..=MARGIN).all(|step| {
                [at - step, at + step, at - step * GRID, at + step * GRID]
                    .iter()
                    .all(|near| from[*near].is_some() && to[*near].is_some())
            })
        };
        let value = |picture: &[Option<f32>], at: usize| picture[at].unwrap_or(0.0);
        let (mut across, mut power) = (0.0, 0.0);
        for at in (MARGIN + 1) * GRID..GRID * GRID - (MARGIN + 1) * GRID {
            if columns.contains(&(at % GRID)) && inside(at) {
                let slope = (value(from, at + 1) + value(to, at + 1)
                    - value(from, at - 1)
                    - value(to, at - 1))
                    / 4.0;
                across += slope * (value(to, at) - value(from, at));
                power += slope * slope;
            }
        }
        -across / power
    }

    #[test]
    fn the_coin_moves_one_way_all_the_way_round() {
        const STEP: usize = 3;
        let mut against = Vec::new();
        for frame in (0..FRAMES).step_by(STEP) {
            let middle = frame_angle(frame) + frame_angle(STEP) / 2.0;
            let motion = net_motion(&field(frame), &field(frame + STEP));
            if cosf(middle).abs() > 0.25 && motion <= 0.0 {
                against.push((frame, motion));
            }
        }
        assert!(against.is_empty(), "moved backwards at {against:?}");
    }

    /// Seen from above, the near half of a turned coin passes lower than the far half. This is
    /// what settles the direction: a coin seen straight on looks the same turning either way.
    #[test]
    fn the_near_side_passes_below_the_far_side() {
        for frame in (6..FRAMES / 4 - 3).step_by(3) {
            let picture = field(frame);
            let height = |columns: core::ops::Range<usize>| {
                let (mut sum, mut count) = (0, 0);
                for (at, spot) in picture.iter().enumerate() {
                    if columns.contains(&(at % GRID)) && spot.is_some() {
                        sum += at / GRID;
                        count += 1;
                    }
                }
                sum as f32 / count as f32
            };
            let (near, far) = (height(0..GRID / 2), height(GRID / 2..GRID));
            assert!(near > far + 1.0, "frame {frame}: {near} against {far}");
        }
    }

    /// The light goes across in the direction of travel, once for each face that passes.
    #[test]
    fn the_shine_crosses_with_the_turn() {
        for middle in [0, FRAMES / 2] {
            let sheen = |step: usize| Spin::new(frame_angle(FRAMES + middle + step - 40)).sheen;
            for step in 0..79 {
                assert!(sheen(step + 1) > sheen(step), "{middle} {step}");
            }
        }
    }

    #[test]
    fn the_slowest_steps_stay_far_from_a_half_turn() {
        let step = 360.0 * 1_000.0 / (TURN_MS as f32 * MIN_FPS as f32);
        assert!(step <= 15.0, "{step}");
    }

    #[test]
    fn the_coin_stays_inside_the_frame() {
        for frame in 0..FRAMES {
            let spin = Spin::new(frame_angle(frame));
            for step in 0..64 {
                let at = step as f32 / 32.0 - 1.0;
                for (x, y) in [(at, 1.0), (at, -1.0), (1.0, at), (-1.0, at)] {
                    assert!(spin.shade(x, y).is_none(), "frame {frame} at {x} {y}");
                }
            }
        }
    }

    #[test]
    fn the_coin_turns_edge_on() {
        let wide = |frame: usize| {
            let spin = Spin::new(frame_angle(frame));
            (0..100)
                .filter(|step| spin.shade(*step as f32 / 50.0 - 1.0, 0.0).is_some())
                .count()
        };
        assert!(wide(0) > 70);
        assert!(wide(FRAMES / 4) < 25);
    }

    #[test]
    fn the_turn_runs_over_every_frame() {
        assert_eq!(frame_at(0), 0);
        assert_eq!(frame_at(TURN_MS - 1), FRAMES - 1);
        assert_eq!(frame_at(TURN_MS), 0);
        assert_eq!(frame_angle(FRAMES / 2), core::f32::consts::PI);
    }

    #[test]
    fn the_symbol_is_stamped_on_both_faces() {
        // The upper left is clear of the arcs and the stroke's end, the middle is the stroke.
        assert!(stamped(0.0, 0.0));
        assert!(!stamped(0.8, 0.5));
        assert!(stamped(0.3, -0.32) || stamped(-0.3, 0.3));
    }
}
