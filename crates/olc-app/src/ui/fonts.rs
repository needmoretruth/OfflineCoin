//! The fonts, carried inside the program: Inter for Latin and numbers, Noto Sans KR for Hangul,
//! IBM Plex Mono for addresses. `fonts/make-fonts.py` builds them. No system font is read.
use egui::{FontData, FontDefinitions, FontFamily};
use std::sync::Arc;

const INTER: &[u8] = include_bytes!("../../fonts/Inter-Regular.ttf");
const INTER_MEDIUM: &[u8] = include_bytes!("../../fonts/Inter-Medium.ttf");
const INTER_STRONG: &[u8] = include_bytes!("../../fonts/Inter-SemiBold.ttf");
const NOTO: &[u8] = include_bytes!("../../fonts/NotoSansKR-Regular.otf");
const NOTO_MEDIUM: &[u8] = include_bytes!("../../fonts/NotoSansKR-Medium.otf");
const NOTO_STRONG: &[u8] = include_bytes!("../../fonts/NotoSansKR-SemiBold.otf");
const MONO: &[u8] = include_bytes!("../../fonts/PlexMono-Regular.ttf");

const MEDIUM_FAMILY: &str = "medium";
const STRONG_FAMILY: &str = "strong";

pub fn medium() -> FontFamily {
    FontFamily::Name(MEDIUM_FAMILY.into())
}

pub fn strong() -> FontFamily {
    FontFamily::Name(STRONG_FAMILY.into())
}

pub fn definitions() -> FontDefinitions {
    let mut fonts = FontDefinitions::empty();
    let latin = [("inter", INTER), ("inter-medium", INTER_MEDIUM), ("inter-strong", INTER_STRONG), ("mono", MONO)];
    for (name, bytes) in latin {
        fonts.font_data.insert(name.into(), Arc::new(FontData::from_static(bytes)));
    }
    let hangul = [("noto", NOTO), ("noto-medium", NOTO_MEDIUM), ("noto-strong", NOTO_STRONG)];
    for (name, bytes) in hangul {
        fonts.font_data.insert(name.into(), Arc::new(FontData::from_static(bytes)));
    }
    let family = |faces: &[&str]| faces.iter().map(|face| (*face).to_string()).collect::<Vec<_>>();
    fonts.families.insert(FontFamily::Proportional, family(&["inter", "noto"]));
    fonts.families.insert(medium(), family(&["inter-medium", "noto-medium", "noto"]));
    fonts.families.insert(strong(), family(&["inter-strong", "noto-strong", "noto"]));
    fonts.families.insert(FontFamily::Monospace, family(&["mono", "noto"]));
    fonts
}
