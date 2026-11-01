//! What an ESP32 board does until someone chooses otherwise. The firmware, the programs on a
//! computer and the pages they serve all read these, so the numbers live here and nowhere else.

/// Hashes per second: enough to mine, little heat.
pub const BOARD_CAP: u32 = 1_000;
/// Processor speed. It holds the cap with the coin showing.
pub const BOARD_CLOCK_MHZ: u32 = 80;
/// Coin frames per second on the panel.
pub const BOARD_SCREEN_RATE: u32 = 10;
