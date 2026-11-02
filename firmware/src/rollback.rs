//! A board on power alone has nobody to press its reset button or flash it again. A stop restarts
//! the chip, and a new build that stops in its first minute goes back to the build before.
//!
//! The bootloader espflash installs does not roll back, so this program does what one that does
//! would: a build is `New` once an update selects it, `PendingVerify` while its first run is
//! young, and `Valid` after a minute of running (`Store::mark_running_build_good`). A start that
//! finds its build still `PendingVerify` follows one that stopped.
use crate::flash;
use esp_bootloader_esp_idf::ota::OtaImageState;
use esp_bootloader_esp_idf::ota_updater::OtaUpdater;
use esp_bootloader_esp_idf::partitions::{
    read_partition_table, AppPartitionSubType, PartitionType, PARTITION_TABLE_MAX_LEN,
};
use esp_println::println;

/// What the first steps of a start found about the build that runs.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Start {
    /// Started before, or put on by cable.
    Known,
    /// The first start after an update.
    First,
    /// The start before this one stopped before it was through.
    Stopped,
}

fn booted() -> Option<AppPartitionSubType> {
    let mut buffer = [0u8; PARTITION_TABLE_MAX_LEN];
    let found = flash::with(|flash| {
        read_partition_table(flash, &mut buffer)
            .ok()
            .and_then(|table| table.booted_partition().ok().flatten())
            .map(|entry| entry.partition_type())
    });
    match found.flatten() {
        Some(PartitionType::App(slot)) => Some(slot),
        _ => None,
    }
}

/// Runs before anything that could stop the start. Marks a first start, and sends a build whose
/// first start stopped back to the other slot.
pub fn check_start() -> Start {
    let Some(running) = booted() else {
        return Start::Known;
    };
    let mut buffer = [0u8; PARTITION_TABLE_MAX_LEN];
    let start = flash::with(|flash| {
        let Ok(mut updater) = OtaUpdater::new(flash, &mut buffer) else {
            return Start::Known;
        };
        // After a cable install nothing is selected, and the bootloader runs the first slot.
        if updater.selected_partition().ok() != Some(running) {
            return Start::Known;
        }
        match updater.current_ota_state() {
            Ok(OtaImageState::New) => {
                let _ = updater.set_current_ota_state(OtaImageState::PendingVerify);
                Start::First
            }
            Ok(OtaImageState::PendingVerify) => Start::Stopped,
            _ => Start::Known,
        }
    });
    start.unwrap_or(Start::Known)
}

/// The slot the build before this one is in, now marked to start, and the chip restarted.
pub fn go_back() -> ! {
    let mut buffer = [0u8; PARTITION_TABLE_MAX_LEN];
    let back = match booted() {
        Some(AppPartitionSubType::Ota1) => AppPartitionSubType::Ota0,
        _ => AppPartitionSubType::Ota1,
    };
    let _ = flash::with(|flash| {
        let Ok(mut updater) = OtaUpdater::new(flash, &mut buffer) else {
            return;
        };
        let Ok(mut data) = updater.ota_data() else {
            return;
        };
        let _ = data.set_current_ota_state(OtaImageState::Invalid);
        if data.set_current_app_partition(back).is_ok() {
            let _ = data.set_current_ota_state(OtaImageState::Valid);
        }
    });
    println!("rollback: the new build stopped on its first start");
    esp_hal::system::software_reset()
}

/// A stop anywhere restarts the chip, after saying where, instead of leaving the panel lit with
/// its last picture until someone pulls the power.
#[panic_handler]
fn stopped(info: &core::panic::PanicInfo) -> ! {
    println!("panic: {info}");
    for frame in esp_backtrace::Backtrace::capture().frames() {
        println!("  at 0x{:08x}", frame.program_counter());
    }
    esp_hal::system::software_reset()
}
