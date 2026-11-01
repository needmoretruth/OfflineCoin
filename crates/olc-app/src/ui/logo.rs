//! The spinning coin of `olc-logo` in the header. Frames are drawn once and kept, and the window
//! repaints for it only while it turns: while mining with the window in front, or once around
//! when a new block arrives.
use egui::{Color32, ColorImage, Context, Image, Rect, TextureHandle, TextureOptions, Ui, Vec2};
use olc_logo::{frame_angle, Spin, FRAMES, MAX_FPS, TURN_MS};
use std::time::Duration;

/// Twice the shown size, for sharp edges on a scaled screen.
const PIXELS: usize = 72;
const SAMPLES: usize = 3;
const TURN_SECONDS: f64 = TURN_MS as f64 / 1_000.0;
const NEW_BLOCK_TURN_SECONDS: f64 = 1.6;
const FRAME_TIME: Duration = Duration::from_millis(1_000 / MAX_FPS as u64);
const REST_FRAME: usize = FRAMES / 16;

/// Draws one view of the coin. White with coverage as alpha, tinted when painted.
pub fn render(angle: f32, pixels: usize) -> ColorImage {
    render_from(angle, pixels, 0.0)
}

/// `floor` lifts the darkest part of the coin, so it holds up on a light background.
pub fn render_from(angle: f32, pixels: usize, floor: f32) -> ColorImage {
    let spin = Spin::new(angle);
    let mut data = Vec::with_capacity(pixels * pixels);
    let total = (SAMPLES * SAMPLES) as f32;
    for row in 0..pixels {
        for column in 0..pixels {
            let (mut covered, mut light) = (0.0, 0.0);
            for sub_y in 0..SAMPLES {
                for sub_x in 0..SAMPLES {
                    let step = |cell: usize, sub: usize| {
                        let place = cell as f32 + (sub as f32 + 0.5) / SAMPLES as f32;
                        place / pixels as f32 * 2.0 - 1.0
                    };
                    if let Some(shade) = spin.shade(step(column, sub_x), -step(row, sub_y)) {
                        covered += 1.0;
                        light += shade;
                    }
                }
            }
            let alpha = covered / total;
            let shade = if covered > 0.0 { light / covered } else { 0.0 };
            let lifted = floor + (1.0 - floor) * shade.clamp(0.0, 1.0);
            let level = (lifted * 255.0) as u8;
            data.push(Color32::from_rgba_unmultiplied(
                level,
                level,
                level,
                (alpha * 255.0) as u8,
            ));
        }
    }
    ColorImage::new([pixels, pixels], data)
}

pub struct Logo {
    frames: Vec<Option<TextureHandle>>,
    position: f64,
    last_time: f64,
    seen_tip: u64,
    /// Turns still owed for a new block.
    owed: f64,
    floor: f32,
}

impl Default for Logo {
    fn default() -> Self {
        Self {
            frames: vec![None; FRAMES],
            position: 0.0,
            last_time: 0.0,
            seen_tip: 0,
            owed: 0.0,
            floor: 0.0,
        }
    }
}

pub struct LogoState {
    pub mining: bool,
    pub tip_changes: u64,
    pub still: bool,
}

impl Logo {
    fn frame(&mut self, ctx: &Context, index: usize) -> &TextureHandle {
        let floor = self.floor;
        self.frames[index].get_or_insert_with(|| {
            ctx.load_texture(
                format!("coin-{index}"),
                render_from(frame_angle(index), PIXELS, floor),
                TextureOptions::LINEAR,
            )
        })
    }

    fn advance(&mut self, ctx: &Context, state: &LogoState, focused: bool) {
        let now = ctx.input(|input| input.time);
        let passed = (now - self.last_time).clamp(0.0, 0.1);
        self.last_time = now;
        if self.seen_tip != state.tip_changes {
            // Not for the blocks read at start.
            if self.seen_tip != 0 {
                self.owed = 1.0;
            }
            self.seen_tip = state.tip_changes;
        }
        if state.still {
            self.owed = 0.0;
            self.position = REST_FRAME as f64 / FRAMES as f64;
            return;
        }
        let steady = state.mining && focused;
        if !steady && self.owed <= 0.0 {
            return;
        }
        let quick = passed / NEW_BLOCK_TURN_SECONDS;
        let step = match self.owed > 0.0 {
            true => quick,
            false => passed / TURN_SECONDS,
        };
        self.owed -= quick;
        self.position = (self.position + step).fract();
        ctx.request_repaint_after(FRAME_TIME);
    }

    pub fn show(&mut self, ui: &mut Ui, size: f32, look: (Color32, f32), state: &LogoState) {
        let (tint, floor) = look;
        if floor != self.floor {
            self.floor = floor;
            self.frames.fill(None);
        }
        let ctx = ui.ctx().clone();
        let focused = ctx.input(|input| input.viewport().focused.unwrap_or(true));
        self.advance(&ctx, state, focused);
        let index = ((self.position * FRAMES as f64) as usize).min(FRAMES - 1);
        let texture = self.frame(&ctx, index).clone();
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::hover());
        let image = Image::new(&texture).tint(tint);
        image.paint_at(ui, Rect::from_center_size(rect.center(), Vec2::splat(size)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::tokens::{Tokens, DARK, LIGHT};

    /// The brightest and darkest the coin's face can reach on screen, out of 255.
    fn levels(tokens: &Tokens) -> (u32, u32) {
        let image = render_from(frame_angle(REST_FRAME), PIXELS, tokens.logo_floor);
        let tint = u32::from(tokens.logo.r());
        let shown = image.pixels.iter().filter(|pixel| pixel.a() == 255);
        let seen: Vec<u32> = shown.map(|pixel| u32::from(pixel.r()) * tint / 255).collect();
        let lowest = seen.iter().min().copied().unwrap_or(0);
        (lowest, seen.iter().max().copied().unwrap_or(0))
    }

    #[test]
    fn the_coin_stays_a_mid_tone_on_a_light_background() {
        let (lowest, highest) = levels(&LIGHT);
        assert!(lowest >= 70, "{lowest}");
        assert!(highest <= 190, "{highest}");
        assert!(levels(&DARK).0 < lowest);
    }
}
