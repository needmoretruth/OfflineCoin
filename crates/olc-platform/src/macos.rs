//! macOS: `ioreg` tells which serial device belongs to which USB device. The system offers no
//! way to hold a process's memory, so a node only runs at a low priority, and the screen says so.
use crate::{foreign, unix, Enforcement, Limits, Platform, Usage, UsbPort, Place, PlaceKind};
use std::path::{Path, PathBuf};
use std::process::Command;

const LOW_PRIORITY: &str = "10";
const SUPPORT: &str = "Library/Application Support/OLC";

pub struct Os;

fn number_after(line: &str, key: &str) -> Option<u32> {
    line.split_once(&format!("\"{key}\" = "))?.1.trim().parse().ok()
}

fn text_after(line: &str, key: &str) -> Option<String> {
    let value = line.split_once(&format!("\"{key}\" = "))?.1;
    Some(value.trim().trim_matches('"').to_string())
}

/// Reads `ioreg -r -c IOUSBHostDevice -l`: each device lists its numbers, then its children, and
/// the serial device is one of them.
fn ports_in(listing: &str, vendor: u16, product: u16) -> Vec<UsbPort> {
    let (mut found_vendor, mut found_product, mut serial) = (0, 0, String::new());
    let mut ports = Vec::new();
    for line in listing.lines() {
        found_vendor = number_after(line, "idVendor").unwrap_or(found_vendor);
        found_product = number_after(line, "idProduct").unwrap_or(found_product);
        serial = text_after(line, "USB Serial Number").unwrap_or(serial);
        let port = text_after(line, "IOCalloutDevice");
        if let Some(path) = port.filter(|_| {
            found_vendor == u32::from(vendor) && found_product == u32::from(product)
        }) {
            ports.push(UsbPort { path, serial: serial.clone() });
        }
    }
    ports
}

impl Platform for Os {
    fn data_folder(&self) -> Option<PathBuf> {
        Some(unix::home()?.join(SUPPORT))
    }

    fn config_folder(&self) -> Option<PathBuf> {
        Some(unix::home()?.join(SUPPORT))
    }

    fn host_name(&self) -> String {
        let output = Command::new("hostname").arg("-s").output();
        let text = output.map(|output| output.stdout).unwrap_or_default();
        String::from_utf8_lossy(&text).trim().to_string()
    }

    fn device_model(&self) -> String {
        String::new()
    }

    fn usb_serial_ports(&self, vendor: u16, product: u16) -> Vec<UsbPort> {
        let words = ["-r", "-c", "IOUSBHostDevice", "-l", "-w", "0"];
        let output = Command::new("ioreg").args(words).output();
        let text = output.map(|output| output.stdout).unwrap_or_default();
        ports_in(&String::from_utf8_lossy(&text), vendor, product)
    }

    fn keep_lines_on_close(&self, settings: &mut serial2::Settings) {
        unix::keep_lines_on_close(settings);
    }

    fn process_exists(&self, pid: u32) -> bool {
        foreign::usage(pid).is_some()
    }

    fn write_private(&self, path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        unix::write_private(path, bytes)
    }

    fn is_private(&self, path: &Path) -> bool {
        unix::is_private(path)
    }

    fn enforcement(&self) -> Enforcement {
        Enforcement::Priority
    }

    fn limited(&self, _how: Enforcement, _limits: &Limits, program: &str, words: &[String]) -> Command {
        let mut command = Command::new("/usr/bin/nice");
        command.args(["-n", LOW_PRIORITY, program]).args(words);
        command
    }

    fn usage(&self, pid: u32) -> Option<Usage> {
        foreign::usage(pid)
    }

    fn places(&self) -> Vec<Place> {
        let Some(home) = unix::home() else {
            return Vec::new();
        };
        let mut places = crate::usual_places(
            &home,
            [
                (PlaceKind::Desktop, home.join("Desktop")),
                (PlaceKind::Documents, home.join("Documents")),
                (PlaceKind::Downloads, home.join("Downloads")),
            ],
        );
        // The system disk is listed there too, as a link back to the root.
        let drives = crate::drives_in(Path::new("/Volumes"));
        places.extend(drives.into_iter().filter(|drive| !drive.path.is_symlink()));
        places
    }

    fn open_folder(&self, path: &Path) -> std::io::Result<()> {
        Command::new("open").arg(path).spawn().map(drop)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LISTING: &str = r#"
+-o USB2.0 Hub@01100000  <class IOUSBHostDevice>
  | {
  |   "idProduct" = 1234
  |   "idVendor" = 1507
  | }
+-o USB JTAG/serial debug unit@01120000  <class IOUSBHostDevice>
  | {
  |   "idProduct" = 4097
  |   "idVendor" = 12346
  |   "USB Serial Number" = "AC:27:6E:AA:E2:04"
  | }
  +-o IOSerialBSDClient  <class IOSerialBSDClient>
      {
        "IOCalloutDevice" = "/dev/cu.usbmodem1101"
      }
"#;

    #[test]
    fn a_board_is_told_by_its_usb_ids() {
        let ports = ports_in(LISTING, 0x303A, 0x1001);
        assert_eq!(ports.len(), 1);
        assert_eq!(ports[0].path, "/dev/cu.usbmodem1101");
        assert_eq!(ports[0].serial, "AC:27:6E:AA:E2:04");
        assert!(ports_in(LISTING, 0x0403, 0x6001).is_empty());
    }
}
