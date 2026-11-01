//! Every design value of the app. Components take colours and sizes from here and nowhere else.
use egui::Color32;

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

pub const TEXT: f32 = 14.0;
pub const META: f32 = 13.0;
pub const TITLE: f32 = 20.0;
pub const BIG: f32 = 28.0;
pub const SECTION: f32 = 15.0;

pub const HEADER: f32 = 58.0;
pub const SIDEBAR: f32 = 208.0;
pub const SIDE_ROW: f32 = 36.0;
pub const ROW: f32 = 38.0;
pub const CONTROL: f32 = 34.0;
pub const LOGO: f32 = 36.0;
pub const ICON: f32 = 18.0;
pub const FIELD: f32 = 200.0;
/// The widest a page's content grows, so rows stay readable on a large screen.
pub const PAGE_WIDTH: f32 = 1500.0;

pub const RADIUS_CARD: u8 = 10;
pub const RADIUS_CONTROL: u8 = 7;
pub const RADIUS_CHIP: u8 = 5;

pub const GAP: f32 = 8.0;
pub const GAP_WIDE: f32 = 16.0;
pub const PAGE_MARGIN: i8 = 24;
pub const CARD_MARGIN: i8 = 16;

/// Seconds a notice stays in view.
pub const NOTICE_SECONDS: f64 = 7.0;

#[derive(Clone, Copy, Debug)]
pub struct Tokens {
    pub dark: bool,
    pub back_top: Color32,
    pub back_bottom: Color32,
    pub side: Color32,
    pub card_top: Color32,
    pub card_bottom: Color32,
    pub raised: Color32,
    pub hover: Color32,
    pub line: Color32,
    pub line_strong: Color32,
    pub glint: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub logo: Color32,
    /// How far the coin's darkest part is lifted on this background.
    pub logo_floor: f32,
    pub main_top: Color32,
    pub main_bottom: Color32,
    pub main_text: Color32,
    pub good: Color32,
    pub warn: Color32,
    pub bad: Color32,
    pub info: Color32,
    pub shadow: Color32,
}

pub const DARK: Tokens = Tokens {
    dark: true,
    back_top: rgb(0x20, 0x1F, 0x1D),
    back_bottom: rgb(0x17, 0x16, 0x15),
    side: rgb(0x1B, 0x1A, 0x19),
    card_top: rgb(0x2A, 0x29, 0x26),
    card_bottom: rgb(0x23, 0x22, 0x20),
    raised: rgb(0x33, 0x31, 0x2E),
    hover: rgb(0x2F, 0x2D, 0x2A),
    line: rgb(0x38, 0x36, 0x32),
    line_strong: rgb(0x52, 0x4F, 0x49),
    glint: Color32::from_rgba_premultiplied(18, 18, 17, 18),
    text: rgb(0xEE, 0xEB, 0xE5),
    muted: rgb(0xAD, 0xA8, 0x9F),
    logo: rgb(0xEE, 0xEB, 0xE5),
    logo_floor: 0.0,
    main_top: rgb(0xF4, 0xF2, 0xEE),
    main_bottom: rgb(0xD8, 0xD5, 0xCF),
    main_text: rgb(0x1B, 0x1A, 0x18),
    good: rgb(0x5B, 0xC2, 0x7E),
    warn: rgb(0xE3, 0xB3, 0x4B),
    bad: rgb(0xEC, 0x6F, 0x66),
    info: rgb(0x6F, 0xA8, 0xE8),
    shadow: Color32::from_black_alpha(70),
};

pub const LIGHT: Tokens = Tokens {
    dark: false,
    back_top: rgb(0xF6, 0xF6, 0xF5),
    back_bottom: rgb(0xEA, 0xEA, 0xE8),
    side: rgb(0xEF, 0xEF, 0xED),
    card_top: rgb(0xFF, 0xFF, 0xFF),
    card_bottom: rgb(0xF9, 0xF9, 0xF8),
    raised: rgb(0xE9, 0xE9, 0xE7),
    hover: rgb(0xE4, 0xE4, 0xE2),
    line: rgb(0xDD, 0xDD, 0xDA),
    line_strong: rgb(0xB9, 0xB8, 0xB4),
    glint: Color32::from_rgba_premultiplied(120, 120, 120, 120),
    text: rgb(0x1B, 0x1A, 0x18),
    muted: rgb(0x62, 0x5F, 0x5A),
    logo: rgb(0xB4, 0xB1, 0xAB),
    logo_floor: 0.42,
    main_top: rgb(0x38, 0x36, 0x33),
    main_bottom: rgb(0x1F, 0x1E, 0x1C),
    main_text: rgb(0xF8, 0xF7, 0xF5),
    good: rgb(0x1E, 0x7F, 0x45),
    warn: rgb(0x95, 0x68, 0x07),
    bad: rgb(0xC0, 0x35, 0x2B),
    info: rgb(0x1F, 0x66, 0xB8),
    shadow: Color32::from_black_alpha(26),
};
