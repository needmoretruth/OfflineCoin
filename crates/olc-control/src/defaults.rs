//! What an ESP32 board does until someone chooses otherwise. The firmware, the programs on a
//! computer and the pages they serve all read these, so the numbers live here and nowhere else.

/// Hashes per second: enough to mine, little heat.
pub const BOARD_CAP: u32 = 1_000;
/// Processor speed. It holds the cap with the coin showing.
pub const BOARD_CLOCK_MHZ: u32 = 80;
/// Coin frames per second on the panel.
pub const BOARD_SCREEN_RATE: u32 = 10;
/// Panel brightness, 0 to 255: low, so a panel left on lasts.
pub const BOARD_BRIGHT: u8 = 64;
/// Bluetooth, the one way two boards stay in touch with every Wi-Fi radio off.
pub const BOARD_BLUETOOTH: bool = true;
