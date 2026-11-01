//! The processor speed. It is fixed when the chip starts, before the saved setting can be read,
//! so a board that wants another speed restarts once and finds its wish in memory that lasts
//! through a restart.
use crate::store::Settings;
use esp_hal::clock::CpuClock;
use portable_atomic::{AtomicU32, Ordering};

/// The slowest speed that holds the default hash cap with room to spare, and the coolest.
pub const DEFAULT_MHZ: u32 = 80;
/// Tells a saved wish from what power-on leaves in this memory.
const MARK: u32 = 0x4F4C_0000;

#[esp_hal::ram(unstable(rtc_fast, persistent))]
static WISH: AtomicU32 = AtomicU32::new(0);

pub fn is_speed(mhz: u32) -> bool {
    matches!(mhz, 80 | 160 | 240)
}

/// The speed this start uses. The first start after power-on takes the default.
pub fn running_mhz() -> u32 {
    let wish = WISH.load(Ordering::Relaxed);
    match wish.checked_sub(MARK) {
        Some(mhz) if is_speed(mhz) => mhz,
        _ => DEFAULT_MHZ,
    }
}

pub fn cpu_clock(mhz: u32) -> CpuClock {
    match mhz {
        160 => CpuClock::_160MHz,
        240 => CpuClock::_240MHz,
        _ => CpuClock::_80MHz,
    }
}

pub fn saved_mhz(settings: &Settings) -> u32 {
    let saved = settings.clock.parse().ok().filter(|mhz| is_speed(*mhz));
    saved.unwrap_or(DEFAULT_MHZ)
}

pub fn restart_at(mhz: u32) -> ! {
    WISH.store(MARK + mhz, Ordering::Relaxed);
    esp_hal::system::software_reset()
}
