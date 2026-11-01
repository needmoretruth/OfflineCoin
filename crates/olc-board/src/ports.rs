//! Which serial ports are boards. The chip's own USB unit gives the same ids on every system.
use olc_platform::{platform, UsbPort};

/// Espressif's vendor id.
const VENDOR: u16 = 0x303A;
/// The USB serial and debug unit inside an ESP32-S3.
const PRODUCT: u16 = 0x1001;

pub fn board_ports() -> Vec<UsbPort> {
    platform().usb_serial_ports(VENDOR, PRODUCT)
}
