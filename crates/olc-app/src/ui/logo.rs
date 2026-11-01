//! The spinning coin of `olc-logo` in the header. Frames are drawn once and kept, and the window
//! repaints for it only while it turns: while mining with the window in front, or once around
//! when a new block arrives.
use egui::{Color32, ColorImage, Context, Image, Rect, TextureHandle, TextureOptions, Ui, Vec2};
use olc_logo::Spin;
use std::f32::consts::TAU;
use std::time::Duration;

const FRAMES: usize = 120;
/// Twice the shown size, for sharp edges on a scaled screen.
const PIXELS: usize = 72;
const SAMPLES: usize = 3;
const TURN_SECONDS: f64 = 4.0;
const NEW_BLOCK_TURN_SECONDS: f64 = 1.6;
const FRAME_TIME: Duration = Duration::from_millis(33);
const REST_FRAME: usize = FRAMES / 16;

/// Draws one view of the coin. White with coverage as alpha, tinted when painted.
pub fn render(angle: f32, pixels: usize) -> ColorImage {
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
            let level = (shade.clamp(0.0, 1.0) * 255.0) as u8;
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
}

impl Default for Logo {
    fn default() -> Self {
        Self {
            frames: vec![None; FRAMES],
            position: 0.0,
            last_time: 0.0,
            seen_tip: 0,
            owed: 0.0,
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
        self.frames[index].get_or_insert_with(|| {
            let angle = index as f32 / FRAMES as f32 * TAU;
            ctx.load_texture(
                format!("coin-{index}"),
                render(angle, PIXELS),
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

    pub fn show(&mut self, ui: &mut Ui, size: f32, tint: Color32, state: &LogoState) {
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
