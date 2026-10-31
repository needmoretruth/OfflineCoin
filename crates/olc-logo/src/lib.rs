//! The logo: a real 3D coin, stamped with the Wi-Fi symbol crossed out. One renderer serves the
//! terminal, the browser and the OLED, each of which only decides how to show the brightness.
#![no_std]

use libm::{atan2f, cosf, powf, sinf, sqrtf};

const THICKNESS: f32 = 0.17;
const RIM: f32 = 0.87;
const REEDS: f32 = 36.0;
/// The coin stays inside the frame at every angle.
const FIT: f32 = 1.08;

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
    /// Surface direction in the coin's own space.
    normal: [f32; 3],
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
    Hit {
        normal: [0.0, 0.0, z_sign],
        albedo,
    }
}

fn edge(x: f32, y: f32) -> Hit {
    let reed = sinf(atan2f(y, x) * REEDS);
    Hit {
        normal: [x, y, 0.0],
        albedo: 0.62 + 0.38 * reed,
    }
}

/// One rotation of the coin, kept so a frame costs a sine and a cosine once.
#[derive(Clone, Copy)]
pub struct Spin {
    sin: f32,
    cos: f32,
}

impl Spin {
    pub fn new(angle: f32) -> Self {
        Self { sin: sinf(angle), cos: cosf(angle) }
    }

    /// Brightness from 0 to 1 at a point of a frame running from -1 to 1 with y up, or `None`
    /// off the coin.
    pub fn shade(self, x: f32, y: f32) -> Option<f32> {
        let (x, y) = (x * FIT, y * FIT);
        let Self { sin, cos } = self;
        // A ray towards the coin, as a line in the coin's space: s runs towards the viewer.
        let across = |s: f32| x * cos - s * sin;
        let depth = |s: f32| x * sin + s * cos;

        let mut best: Option<(f32, Hit)> = None;
        let mut consider = |s: f32, hit: Hit| {
            if best.as_ref().is_none_or(|(nearest, _)| s > *nearest) {
                best = Some((s, hit));
            }
        };
        if cos.abs() > 1e-4 {
            for side in [1.0, -1.0] {
                let s = (side * THICKNESS - x * sin) / cos;
                let u = across(s);
                if u * u + y * y <= 1.0 {
                    consider(s, face(u, y, side));
                }
            }
        }
        if sin.abs() > 1e-4 && y.abs() <= 1.0 {
            let half = sqrtf(1.0 - y * y);
            for side in [1.0, -1.0] {
                let u = side * half;
                let s = (x * cos - u) / sin;
                if depth(s).abs() <= THICKNESS {
                    consider(s, edge(u, y));
                }
            }
        }
        let (_, hit) = best?;
        let [nx, ny, nz] = hit.normal;
        let normal = [nx * cos + nz * sin, ny, -nx * sin + nz * cos];
        Some(light(normal, hit.albedo))
    }
}

/// See [`Spin::shade`].
pub fn shade(angle: f32, x: f32, y: f32) -> Option<f32> {
    Spin::new(angle).shade(x, y)
}

fn light(normal: [f32; 3], albedo: f32) -> f32 {
    const LIGHT: [f32; 3] = [-0.44, 0.53, 0.72];
    // A weaker light on the other side keeps the symbol visible all the way round.
    const FILL: [f32; 3] = [0.60, 0.20, 0.77];
    const FILL_STRENGTH: f32 = 0.85;
    const HALFWAY: [f32; 3] = [-0.24, 0.29, 0.93];
    let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let diffuse = dot(normal, LIGHT)
        .max(dot(normal, FILL) * FILL_STRENGTH)
        .max(0.0);
    let shine = powf(dot(normal, HALFWAY).max(0.0), 24.0);
    ((0.30 + 0.62 * diffuse) * albedo + 0.55 * shine).clamp(0.0, 1.0)
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
    use super::*;

    #[test]
    fn the_coin_is_round_and_turns_edge_on() {
        assert!(shade(0.0, 0.0, 0.8).is_some());
        assert!(shade(0.0, 0.9, 0.9).is_none());
        let edge_on = core::f32::consts::FRAC_PI_2;
        assert!(shade(edge_on, 0.5, 0.0).is_none());
        assert!(shade(edge_on, 0.1, 0.0).is_some());
    }

    #[test]
    fn the_symbol_is_stamped_on_both_faces() {
        // The upper left is clear of the arcs and the stroke's end, the middle is the stroke.
        assert!(stamped(0.0, 0.0));
        assert!(!stamped(0.8, 0.5));
        assert!(stamped(0.3, -0.32) || stamped(-0.3, 0.3));
    }
}
