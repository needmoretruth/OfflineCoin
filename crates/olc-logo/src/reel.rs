//! One turn of the coin as one-bit frames, drawn a few rows at a time in spare moments. A turn takes
//! too long to draw at once, so the reel works while it fills, showing what exists so far.
use crate::{frame_angle, lit, Spin, FRAMES};
use alloc::vec;
use alloc::vec::Vec;

/// Frames are drawn in this stride, so the first few are spread over the whole turn.
const STRIDE: usize = 113;

pub struct Reel {
    radius: i32,
    /// One word for each row of each frame, the leftmost pixel in the lowest bit.
    rows: Vec<u64>,
    drawn: Vec<bool>,
    /// How many frames are finished, and how many rows of the next one.
    finished: usize,
    next_row: i32,
}

impl Reel {
    /// A reel of frames `radius * 2` wide. That can be at most 64.
    pub fn new(radius: i32) -> Self {
        Self {
            radius,
            rows: vec![0; FRAMES * Self::height(radius)],
            drawn: vec![false; FRAMES],
            finished: 0,
            next_row: 0,
        }
    }

    fn height(radius: i32) -> usize {
        radius as usize * 2
    }

    pub fn width(&self) -> i32 {
        self.radius * 2
    }

    pub fn is_full(&self) -> bool {
        self.finished == FRAMES
    }

    /// Draws up to `count` more rows.
    pub fn work(&mut self, count: usize) {
        for _ in 0..count {
            if self.is_full() {
                return;
            }
            self.draw_row();
        }
    }

    fn draw_row(&mut self) {
        let frame = self.finished * STRIDE % FRAMES;
        let spin = Spin::new(frame_angle(frame));
        let height = Self::height(self.radius);
        let mut bits = 0;
        for x in -self.radius..self.radius {
            if lit(spin, x, self.next_row - self.radius, self.radius) {
                bits |= 1 << (x + self.radius);
            }
        }
        self.rows[frame * height + self.next_row as usize] = bits;
        self.next_row += 1;
        if self.next_row as usize == height {
            self.next_row = 0;
            self.drawn[frame] = true;
            self.finished += 1;
        }
    }

    /// The rows of a frame, or of the nearest drawn one before it. `None` until one is drawn.
    pub fn frame(&self, frame: usize) -> Option<&[u64]> {
        let found = (0..FRAMES)
            .map(|back| (frame + FRAMES - back) % FRAMES)
            .find(|candidate| self.drawn[*candidate])?;
        let height = Self::height(self.radius);
        Some(&self.rows[found * height..][..height])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_shows_until_a_frame_is_done() {
        let mut reel = Reel::new(4);
        assert!(reel.frame(0).is_none());
        reel.work(7);
        assert!(reel.frame(0).is_none());
        reel.work(1);
        assert!(reel.frame(0).is_some());
    }

    #[test]
    fn what_exists_is_spread_over_the_turn() {
        let mut reel = Reel::new(2);
        reel.work(6 * 4);
        let mut drawn: Vec<usize> = (0..FRAMES).filter(|frame| reel.drawn[*frame]).collect();
        drawn.sort();
        let widest = drawn.windows(2).map(|pair| pair[1] - pair[0]).max();
        assert!(widest.is_some_and(|gap| gap < FRAMES / 3), "{drawn:?}");
    }

    /// Both faces carry the same stamp, so the second half of a turn may repeat the first.
    #[test]
    fn every_frame_of_a_face_is_different() {
        let mut reel = Reel::new(19);
        reel.work(FRAMES * 38);
        assert!(reel.is_full());
        let frames: Vec<&[u64]> = (0..FRAMES).filter_map(|frame| reel.frame(frame)).collect();
        assert_eq!(frames.len(), FRAMES);
        for (index, frame) in frames.iter().enumerate() {
            let rest = (1..FRAMES / 2).map(|step| frames[(index + step) % FRAMES]);
            assert_eq!(rest.filter(|other| other == frame).count(), 0, "{index}");
        }
    }

    #[test]
    fn a_full_reel_stops_working() {
        let mut reel = Reel::new(1);
        reel.work(FRAMES * 2 + 5);
        assert!(reel.is_full());
    }
}
