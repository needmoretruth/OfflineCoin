//! Puts this program's firmware on a board through its cable: flashing, trusting the control key
//! and naming it. A board's wallet and chain live in partitions flashing does not touch.
use crate::board::{Board, UsbBoard};
use crate::control_key::config_folder;
use olc_control::line::{quoted, MOST_NAME_BYTES};
use olc_link_serial::port_is_here;
use olc_platform::{platform, tool, usb_is_off};
use std::fs;
use std::path::Path;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

const PARTITIONS: &str = include_str!("../../../firmware/partitions.csv");
const FLASH_SIZE: &str = "16mb";
/// A board with a long chain takes this long to read it back from flash.
const START_WAIT: Duration = Duration::from_secs(90);
const PORT_WAIT: Duration = Duration::from_secs(30);
const STEP: Duration = Duration::from_millis(500);

/// The flashing tool, which release builds carry beside the app.
fn espflash() -> Command {
    let mut command = Command::new(tool("espflash"));
    platform().hide_window(&mut command);
    command
}

fn run(command: &mut Command) -> Result<(), String> {
    if usb_is_off() {
        return Err("USB is switched off here".into());
    }
    let output = command
        .output()
        .map_err(|error| format!("espflash: {error}"))?;
    if output.status.success() {
        return Ok(());
    }
    let text = String::from_utf8_lossy(&output.stderr);
    Err(format!("espflash failed: {}", text.trim()))
}

fn wait_for_port(port: &str) -> Result<(), String> {
    let deadline = Instant::now() + PORT_WAIT;
    while !port_is_here(port) {
        if Instant::now() > deadline {
            return Err(format!("{port} did not come back."));
        }
        thread::sleep(STEP);
    }
    Ok(())
}

fn flash(port: &str, firmware: &Path) -> Result<(), String> {
    let table = config_folder()?.join("partitions.csv");
    fs::write(&table, PARTITIONS).map_err(|error| error.to_string())?;
    run(espflash()
        .args(["flash", "--chip", "esp32s3", "--non-interactive"])
        .args(["--port", port, "--flash-size", FLASH_SIZE])
        .arg("--partition-table")
        .arg(&table)
        .args(["--target-app-partition", "ota_0"])
        .args(["--erase-parts", "otadata"])
        .args(["--after", "watchdog-reset"])
        .arg(firmware))
}

/// The bytes of the update slot for `firmware`, an ELF file.
pub fn app_image(firmware: &Path) -> Result<Vec<u8>, String> {
    let out = config_folder()?.join("update.bin");
    run(espflash()
        .args(["save-image", "--chip", "esp32s3"])
        .args(["--flash-size", FLASH_SIZE])
        .arg(firmware)
        .arg(&out))?;
    let image = fs::read(&out).map_err(|error| error.to_string())?;
    let _ = fs::remove_file(&out);
    Ok(image)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Waits for the board to start. The port comes and goes while it restarts, so each try opens it
/// anew.
fn started_board(port: &str) -> Result<Board, String> {
    let deadline = Instant::now() + START_WAIT;
    loop {
        let opened = wait_for_port(port).and_then(|()| UsbBoard::open(port));
        let tried = opened.and_then(|link| {
            let mut board = Board::new("new", Box::new(link));
            board.status().map(|_| board)
        });
        if tried.is_ok() || Instant::now() > deadline {
            return tried;
        }
        thread::sleep(STEP);
    }
}

/// Flashes the board on `port`, gives it the control key and a name.
pub fn install(
    port: &str,
    name: &str,
    firmware: &Path,
    key: olc_crypto::PublicKey,
) -> Result<(), String> {
    if name.trim().is_empty() || name.len() > MOST_NAME_BYTES {
        return Err(format!("A board name has 1 to {MOST_NAME_BYTES} bytes."));
    }
    flash(port, firmware)?;
    let mut board = started_board(port)?;
    board.command(&format!("trust {}", hex(&key)), "trusted")?;
    board.command(&format!("name {}", quoted(name)), "named")
}
