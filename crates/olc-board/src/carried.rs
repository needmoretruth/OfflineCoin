//! The firmware this program carries, so it alone can install and update boards.
use crate::control_key::config_folder;
use std::fs;
use std::path::{Path, PathBuf};

const CARRIED: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/firmware.elf"));
const UNPACKED: &str = "firmware.elf";
const FIRMWARE_NAME: &str = "olc-firmware";
const SOURCE_BUILD: &str = "../../firmware/target/xtensa-esp32s3-none-elf/release/olc-firmware";

fn unpack() -> Option<PathBuf> {
    if CARRIED.is_empty() {
        return None;
    }
    let path = config_folder().ok()?.join(UNPACKED);
    let same = fs::read(&path).is_ok_and(|saved| saved == CARRIED);
    (same || fs::write(&path, CARRIED).is_ok()).then_some(path)
}

/// The firmware to install: `chosen`, what this program carries, the file beside it, or what
/// this source tree last built.
pub fn firmware_path(chosen: Option<&str>) -> PathBuf {
    if let Some(chosen) = chosen {
        return PathBuf::from(chosen);
    }
    let beside_program = std::env::current_exe()
        .ok()
        .and_then(|program| Some(program.parent()?.join(FIRMWARE_NAME)));
    unpack()
        .or(beside_program.filter(|file| file.exists()))
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join(SOURCE_BUILD))
}

/// The build boards are kept on, if this program carries one.
pub fn build() -> Option<&'static str> {
    (!CARRIED.is_empty()).then_some(env!("OLC_CARRIED_BUILD"))
}
