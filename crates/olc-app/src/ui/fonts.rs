//! The fonts, carried inside the program: IBM Plex Sans KR for Latin and Hangul together, and
//! IBM Plex Mono for addresses. No system font is read.
use egui::{FontData, FontDefinitions, FontFamily};
use std::sync::Arc;

const REGULAR: &[u8] = include_bytes!("../../fonts/PlexKR-Regular.ttf");
const STRONG: &[u8] = include_bytes!("../../fonts/PlexKR-SemiBold.ttf");
const MONO: &[u8] = include_bytes!("../../fonts/PlexMono-Regular.ttf");

pub const STRONG_FAMILY: &str = "strong";

pub fn strong() -> FontFamily {
    FontFamily::Name(STRONG_FAMILY.into())
}

pub fn definitions() -> FontDefinitions {
    let mut fonts = FontDefinitions::empty();
    for (name, bytes) in [("regular", REGULAR), ("strong", STRONG), ("mono", MONO)] {
        let data = Arc::new(FontData::from_static(bytes));
        fonts.font_data.insert(name.into(), data);
    }
    fonts
        .families
        .insert(FontFamily::Proportional, vec!["regular".into()]);
    fonts.families.insert(strong(), vec!["strong".into()]);
    fonts
        .families
        .insert(FontFamily::Monospace, vec!["mono".into(), "regular".into()]);
    fonts
}
