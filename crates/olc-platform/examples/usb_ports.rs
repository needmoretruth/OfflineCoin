//! What this system reports for a USB id, for checking a new platform file. It opens nothing.
//! `usb_ports 303A 1001` lists the ports of ESP32-S3 boards.
use olc_platform::platform;

fn main() {
    let mut words = std::env::args().skip(1);
    let mut id = || u16::from_str_radix(&words.next().unwrap_or_default(), 16).unwrap_or(0);
    let (vendor, product) = (id(), id());
    for port in platform().usb_serial_ports(vendor, product) {
        println!("{} {}", port.path, port.serial);
    }
}
