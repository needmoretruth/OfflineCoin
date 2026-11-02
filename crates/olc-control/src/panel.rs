//! How a panel that runs for years is kept from burning in. An OLED pixel ages with its brightness
//! and its hours lit, and the picture can only move by a pixel, so a stroke two pixels wide keeps
//! one pixel lit. The brightness does the rest of the work.
use crate::defaults::BOARD_BRIGHT;

/// How long a brighter panel than the default stays so with nobody touching the board.
pub const REST_AFTER_MS: u64 = 60 * 60_000;

/// The brightness to show now. A person who turned it up gets it while they are there, and it
/// stays their setting, but a board left alone goes down to the default and not past it.
pub fn shown_bright(chosen: u8, quiet_ms: u64) -> u8 {
    match quiet_ms >= REST_AFTER_MS {
        true => chosen.min(BOARD_BRIGHT),
        false => chosen,
    }
}

/// The brightness until the node is running. A board that keeps restarting never gets far enough
/// to count as left alone, and shows the same logo every time.
pub fn start_bright(chosen: u8) -> u8 {
    chosen.min(BOARD_BRIGHT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_brighter_panel_rests_when_left_alone() {
        assert_eq!(shown_bright(255, 0), 255);
        assert_eq!(shown_bright(255, REST_AFTER_MS - 1), 255);
        assert_eq!(shown_bright(255, REST_AFTER_MS), BOARD_BRIGHT);
        assert_eq!(shown_bright(255, u64::MAX), BOARD_BRIGHT);
    }

    #[test]
    fn a_dimmer_panel_stays_as_it_was_chosen() {
        assert_eq!(shown_bright(10, REST_AFTER_MS * 9), 10);
        assert_eq!(start_bright(10), 10);
    }

    #[test]
    fn a_start_is_never_brighter_than_the_default() {
        assert_eq!(start_bright(255), BOARD_BRIGHT);
    }
}
