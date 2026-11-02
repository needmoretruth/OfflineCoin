//! The printer's automatic choice reads the register holding the cable's incoming bytes, on this
//! chip, and every print then lost a byte of what the computer was sending.

#[test]
fn the_firmware_prints_to_the_cable_without_the_automatic_choice() {
    let manifest = include_str!("../../../firmware/Cargo.toml");
    let printer = manifest
        .lines()
        .find(|line| line.starts_with("esp-println"))
        .expect("the firmware uses esp-println");
    assert!(printer.contains("default-features = false"), "{printer}");
    assert!(printer.contains("\"jtag-serial\""), "{printer}");
    assert!(!printer.contains("\"auto\""), "{printer}");
}
