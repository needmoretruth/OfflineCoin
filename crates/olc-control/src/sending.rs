//! What a board keeps while it sends its own image to another: which pieces are still wanted and
//! how many it may send before it asks again, so a burst never outruns the other board.
use crate::update::PIECE;
use alloc::vec::Vec;

const WINDOW: u32 = 16;

pub struct Sending {
    session: u32,
    size: u32,
    /// Stretches still wanted, as (first piece, how many).
    runs: Vec<(u32, u32)>,
    credit: u32,
    arrived: bool,
}

impl Sending {
    pub fn new(session: u32, size: u32) -> Self {
        Self {
            session,
            size,
            runs: Vec::new(),
            credit: 0,
            arrived: false,
        }
    }

    pub fn session(&self) -> u32 {
        self.session
    }

    /// True while pieces are still on their way. The other board then writes on its own.
    pub fn moving(&self) -> bool {
        !self.arrived
    }

    pub fn size(&self) -> u32 {
        self.size
    }

    /// The other board's word on what it lacks. True when it holds the whole image in flash.
    pub fn heard(&mut self, done: u32, runs: Vec<(u32, u32)>) -> bool {
        self.runs = runs;
        self.credit = WINDOW;
        self.arrived = done >= self.size;
        done >= self.size.saturating_mul(2)
    }

    /// The next piece to send, as (index, offset, length).
    pub fn next_piece(&mut self) -> Option<(u32, u32, usize)> {
        if self.credit == 0 {
            return None;
        }
        let (first, count) = self.runs.first_mut()?;
        let index = *first;
        *first += 1;
        *count -= 1;
        if *count == 0 {
            self.runs.remove(0);
        }
        self.credit -= 1;
        let offset = index.checked_mul(PIECE as u32)?;
        let length = self.size.checked_sub(offset)?.min(PIECE as u32) as usize;
        Some((index, offset, length))
    }

    /// Nothing more goes out until the other board has answered.
    pub fn must_ask(&self) -> bool {
        self.credit == 0 || self.runs.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: u32 = PIECE as u32 * 3 + 7;

    #[test]
    fn nothing_goes_out_before_the_other_board_has_answered() {
        let mut sending = Sending::new(1, SIZE);
        assert!(sending.must_ask());
        assert_eq!(sending.next_piece(), None);
    }

    #[test]
    fn only_the_wanted_pieces_go_out_and_the_last_is_short() {
        let mut sending = Sending::new(1, SIZE);
        assert!(!sending.heard(0, alloc::vec![(1, 1), (3, 1)]));
        assert_eq!(sending.next_piece(), Some((1, PIECE as u32, PIECE)));
        assert_eq!(sending.next_piece(), Some((3, PIECE as u32 * 3, 7)));
        assert_eq!(sending.next_piece(), None);
        assert!(sending.must_ask());
    }

    #[test]
    fn a_window_ends_in_a_question() {
        let mut sending = Sending::new(1, u32::MAX / 4);
        sending.heard(0, alloc::vec![(0, 1_000)]);
        for _ in 0..WINDOW {
            assert!(sending.next_piece().is_some());
        }
        assert!(sending.must_ask());
        assert_eq!(sending.next_piece(), None);
    }

    #[test]
    fn a_piece_past_the_end_is_never_read() {
        let mut sending = Sending::new(1, SIZE);
        sending.heard(0, alloc::vec![(9, 1)]);
        assert_eq!(sending.next_piece(), None);
    }

    #[test]
    fn it_ends_when_the_whole_image_is_in_flash() {
        let mut sending = Sending::new(1, SIZE);
        assert!(sending.moving());
        assert!(!sending.heard(SIZE, Vec::new()));
        assert!(!sending.moving());
        assert!(sending.heard(SIZE * 2, Vec::new()));
    }
}
