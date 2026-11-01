//! Every design value of the app. Components take colours and sizes from here and nowhere else.
//! `DESIGN.md` beside the crate says why they are what they are.
use egui::Color32;

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

pub const AMOUNT: f32 = 44.0;
pub const SLIP_AMOUNT: f32 = 32.0;
pub const TITLE: f32 = 20.0;
pub const SECTION: f32 = 15.0;
pub const TEXT: f32 = 14.0;
pub const META: f32 = 13.0;

pub const RAIL: f32 = 200.0;
pub const RAIL_NARROW: f32 = 164.0;
pub const RAIL_ROW: f32 = 38.0;
pub const ROW: f32 = 38.0;
pub const CONTROL: f32 = 34.0;
pub const LOGO: f32 = 30.0;
pub const ICON: f32 = 18.0;
pub const FIELD: f32 = 280.0;
pub const FACTS: f32 = 260.0;
/// The widest a page's content grows, so rows stay readable on a large screen.
pub const PAGE_WIDTH: f32 = 1240.0;
/// Below this a page stacks what it would set side by side.
pub const COMPACT: f32 = 760.0;
/// Below this the side rail narrows.
pub const NARROW_WINDOW: f32 = 1000.0;
pub const SLIP_WIDTH: f32 = 440.0;
pub const SHEET_WIDTH: f32 = 520.0;
pub const SHEET_WIDE: f32 = 640.0;

pub const RADIUS_TAG: u8 = 6;
pub const RADIUS_CONTROL: u8 = 8;
pub const RADIUS_BUTTON: u8 = 10;
pub const RADIUS_CARD: u8 = 12;
pub const RADIUS_SHEET: u8 = 16;

pub const GAP: f32 = 8.0;
pub const GAP_WIDE: f32 = 16.0;
pub const GAP_PAGE: f32 = 24.0;
pub const PAGE_MARGIN: i8 = 32;
pub const PAGE_MARGIN_NARROW: i8 = 20;
pub const CARD_MARGIN: i8 = 16;

/// A press takes this long.
pub const PRESS_SECONDS: f32 = 0.12;
/// The one soft change: the balance rolling and a new line lighting.
pub const STAMP_SECONDS: f32 = 0.32;
pub const PRESS_SCALE: f32 = 0.98;

/// Seconds a good notice stays in view.
pub const NOTICE_SECONDS: f64 = 7.0;
/// A failure stays longer, since it asks the reader to do something.
pub const FAILURE_SECONDS: f64 = 14.0;

#[derive(Clone, Copy, Debug)]
pub struct Tokens {
    pub dark: bool,
    pub back_glow: Color32,
    pub back_mid: Color32,
    pub back_floor: Color32,
    pub card_top: Color32,
    pub card_bottom: Color32,
    pub float_top: Color32,
    pub float_bottom: Color32,
    /// One wash layer for hover.
    pub hover: Color32,
    pub line: Color32,
    pub line_strong: Color32,
    pub glint: Color32,
    pub text: Color32,
    pub soft: Color32,
    pub muted: Color32,
    pub ink_top: Color32,
    pub ink_bottom: Color32,
    pub logo: Color32,
    /// How far the coin's darkest part is lifted on this background.
    pub logo_floor: f32,
    pub main_top: Color32,
    pub main_bottom: Color32,
    pub main_text: Color32,
    pub danger_top: Color32,
    pub danger_bottom: Color32,
    pub danger_text: Color32,
    pub good: Color32,
    pub warn: Color32,
    pub bad: Color32,
    pub info: Color32,
    pub shadow: Color32,
    pub scrim: Color32,
}

pub const DARK: Tokens = Tokens {
    dark: true,
    back_glow: rgb(0x1A, 0x1E, 0x27),
    back_mid: rgb(0x0E, 0x10, 0x14),
    back_floor: rgb(0x0A, 0x0C, 0x10),
    card_top: rgb(0x18, 0x1C, 0x23),
    card_bottom: rgb(0x13, 0x16, 0x1C),
    float_top: rgb(0x20, 0x25, 0x2E),
    float_bottom: rgb(0x18, 0x1C, 0x23),
    hover: Color32::from_rgba_premultiplied(12, 12, 12, 12),
    line: Color32::from_rgba_premultiplied(18, 18, 18, 18),
    line_strong: Color32::from_rgba_premultiplied(40, 40, 40, 40),
    glint: Color32::from_rgba_premultiplied(15, 15, 15, 15),
    text: rgb(0xE6, 0xE8, 0xEC),
    soft: rgb(0xB3, 0xB9, 0xC4),
    muted: rgb(0x8B, 0x91, 0x9C),
    ink_top: rgb(0xFF, 0xFF, 0xFF),
    ink_bottom: rgb(0xA9, 0xB0, 0xBD),
    logo: rgb(0xE6, 0xE8, 0xEC),
    logo_floor: 0.0,
    main_top: rgb(0xFF, 0xFF, 0xFF),
    main_bottom: rgb(0xD9, 0xDD, 0xE4),
    main_text: rgb(0x0E, 0x10, 0x14),
    danger_top: rgb(0xE5, 0x4B, 0x4B),
    danger_bottom: rgb(0xC2, 0x35, 0x2B),
    danger_text: rgb(0xFF, 0xFF, 0xFF),
    good: rgb(0x30, 0xA4, 0x6C),
    warn: rgb(0xF5, 0xC2, 0x42),
    bad: rgb(0xFF, 0x5D, 0x5D),
    info: rgb(0x5A, 0xA9, 0xFF),
    shadow: Color32::from_black_alpha(150),
    scrim: Color32::from_black_alpha(150),
};

pub const LIGHT: Tokens = Tokens {
    dark: false,
    back_glow: rgb(0xFF, 0xFF, 0xFF),
    back_mid: rgb(0xF4, 0xF5, 0xF7),
    back_floor: rgb(0xEC, 0xEE, 0xF2),
    card_top: rgb(0xFF, 0xFF, 0xFF),
    card_bottom: rgb(0xFA, 0xFB, 0xFC),
    float_top: rgb(0xFF, 0xFF, 0xFF),
    float_bottom: rgb(0xFA, 0xFB, 0xFC),
    hover: Color32::from_rgba_premultiplied(0, 0, 1, 10),
    line: Color32::from_rgba_premultiplied(1, 1, 2, 20),
    line_strong: Color32::from_rgba_premultiplied(2, 3, 4, 46),
    glint: Color32::from_rgba_premultiplied(255, 255, 255, 255),
    text: rgb(0x1A, 0x1D, 0x22),
    soft: rgb(0x4A, 0x50, 0x59),
    muted: rgb(0x5A, 0x62, 0x70),
    ink_top: rgb(0x0E, 0x10, 0x14),
    ink_bottom: rgb(0x4A, 0x50, 0x59),
    logo: rgb(0xB4, 0xB1, 0xAB),
    logo_floor: 0.42,
    main_top: rgb(0x2A, 0x2F, 0x38),
    main_bottom: rgb(0x0E, 0x10, 0x14),
    main_text: rgb(0xFF, 0xFF, 0xFF),
    danger_top: rgb(0xD2, 0x41, 0x37),
    danger_bottom: rgb(0xB0, 0x2C, 0x23),
    danger_text: rgb(0xFF, 0xFF, 0xFF),
    good: rgb(0x16, 0x7A, 0x3B),
    warn: rgb(0x9A, 0x67, 0x00),
    bad: rgb(0xC2, 0x35, 0x2B),
    info: rgb(0x25, 0x63, 0xC9),
    shadow: Color32::from_rgba_premultiplied(2, 2, 3, 46),
    scrim: Color32::from_rgba_premultiplied(5, 6, 8, 96),
};
