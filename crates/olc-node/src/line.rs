//! A link over any plain stream of bytes between two nodes, such as jumper wires. Messages travel
//! as `#hex` lines like the ones on the USB cable, so noise costs one line and never the framing.
//! Each ends in a two-byte check, because a wire loses bits and a cable does not.
use crate::medium::Medium;
use crate::neighbours::CLOSE_MS;
use crate::runner::{Link, Target};
use alloc::vec::Vec;

/// A line longer than this is noise. It holds the largest block a board sends, written as hex.
const MOST_BYTES: usize = 64 * 1024;

/// The wire itself. Neither direction may wait for the far end.
pub trait Pipe: Send {
    fn write(&mut self, bytes: &[u8]);
    /// Fills `into` with what has arrived, and says how much that was.
    fn read(&mut self, into: &mut [u8]) -> usize;
    /// A line came in spoiled, which tells how clean the wire is.
    fn spoiled_line(&mut self) {}
    /// A whole line came in, so the far end is there.
    fn heard_line(&mut self) {}
}

/// Wires can run straight or crossed, and nothing tells which, so each end does what Ethernet's
/// Auto MDI-X does: it talks on one wire, listens on the other, and swaps after a random stay
/// until it hears a line. Then it stays, so a link that works is never tried again, and a far end
/// busy for some seconds finds it where it was.
#[derive(Clone, Copy, Debug)]
pub struct Hunt {
    /// Which of the two ways this end is using.
    pub way: bool,
    pub swaps: u32,
    heard_ms: Option<u64>,
    swap_at_ms: u64,
}

/// Longer than a Hello, so one way always gets a whole Hello to land.
const SHORTEST_STAY_MS: u64 = 1_200;
const STAY_SPREAD_MS: u64 = 1_400;

impl Hunt {
    pub fn new(random: u32) -> Self {
        Self { way: random & 1 == 1, swaps: 0, heard_ms: None, swap_at_ms: 0 }
    }

    pub fn heard(&mut self, now: u64) {
        self.heard_ms = Some(now);
    }

    pub fn linked(&self, now: u64) -> bool {
        self.heard_ms.is_some_and(|heard| now.saturating_sub(heard) < CLOSE_MS)
    }

    /// Says whether to swap now. Called often, with a fresh random number.
    pub fn turn(&mut self, now: u64, random: u32) -> bool {
        if self.linked(now) || now < self.swap_at_ms {
            return false;
        }
        let first = self.swap_at_ms == 0;
        self.swap_at_ms = now + SHORTEST_STAY_MS + u64::from(random) % STAY_SPREAD_MS;
        if first {
            return false;
        }
        self.way = !self.way;
        self.swaps += 1;
        true
    }

    /// Leaves the link it has, to prove that it finds it again.
    pub fn force_swap(&mut self, now: u64) {
        self.heard_ms = None;
        self.swap_at_ms = now + SHORTEST_STAY_MS;
        self.way = !self.way;
        self.swaps += 1;
    }
}

fn digit(byte: u8) -> Option<u8> {
    (byte as char).to_digit(16).map(|value| value as u8)
}

fn hex_of(value: u8) -> u8 {
    b"0123456789abcdef"[usize::from(value)]
}

/// Fletcher's 16-bit sum, which notices a flipped bit and a line cut short.
fn check_of(bytes: &[u8]) -> [u8; 2] {
    let (mut low, mut high) = (0u16, 0u16);
    for byte in bytes {
        low = (low + u16::from(*byte)) % 255;
        high = (high + low) % 255;
    }
    [low as u8, high as u8]
}

pub fn encode(message: &[u8]) -> Vec<u8> {
    let mut line = Vec::with_capacity(message.len() * 2 + 7);
    line.extend_from_slice(b"\n#");
    for byte in message.iter().chain(&check_of(message)) {
        line.push(hex_of(byte >> 4));
        line.push(hex_of(byte & 15));
    }
    line.push(b'\n');
    line
}

/// Collects bytes into messages.
#[derive(Default)]
pub struct Lines {
    line: Vec<u8>,
    spoiled: bool,
}

impl Lines {
    /// Takes one byte, and says what it finished.
    pub fn push(&mut self, byte: u8) -> Heard {
        if byte != b'\n' {
            match self.line.len() < MOST_BYTES {
                true => self.line.push(byte),
                false => self.spoiled = true,
            }
            return Heard::Nothing;
        }
        let spoiled = core::mem::take(&mut self.spoiled);
        let line = core::mem::take(&mut self.line);
        if line.is_empty() && !spoiled {
            return Heard::Nothing;
        }
        match good_message(&line).filter(|_| !spoiled) {
            Some(message) => Heard::Message(message),
            None => Heard::Spoiled,
        }
    }
}

/// What one byte finished.
#[derive(Debug, PartialEq, Eq)]
pub enum Heard {
    Nothing,
    Message(Vec<u8>),
    Spoiled,
}

fn good_message(line: &[u8]) -> Option<Vec<u8>> {
    let hex = line.strip_prefix(b"#")?;
    if hex.len() % 2 != 0 {
        return None;
    }
    let bytes: Option<Vec<u8>> =
        hex.chunks(2).map(|pair| Some(digit(pair[0])? << 4 | digit(pair[1])?)).collect();
    let mut bytes = bytes?;
    let check = bytes.split_off(bytes.len().checked_sub(2)?);
    (check == check_of(&bytes)).then_some(bytes)
}

pub struct LineLink<P> {
    pipe: P,
    lines: Lines,
    bytes: [u8; 256],
    /// Bytes read but not looked at yet.
    waiting: core::ops::Range<usize>,
}

impl<P: Pipe> LineLink<P> {
    pub fn new(pipe: P) -> Self {
        Self { pipe, lines: Lines::default(), bytes: [0; 256], waiting: 0..0 }
    }
}

impl<P: Pipe> Link for LineLink<P> {
    fn medium(&self) -> Medium {
        Medium::Wire
    }

    /// One wire has one node at its far end, so every target is that node.
    fn send(&mut self, target: Target, message: &[u8]) {
        if target != Target::AllExcept(0) {
            self.pipe.write(&encode(message));
        }
    }

    fn receive(&mut self) -> Option<(u32, Vec<u8>)> {
        loop {
            if self.waiting.is_empty() {
                let size = self.pipe.read(&mut self.bytes);
                if size == 0 {
                    return None;
                }
                self.waiting = 0..size;
            }
            for at in self.waiting.by_ref() {
                match self.lines.push(self.bytes[at]) {
                    Heard::Message(message) => {
                        self.pipe.heard_line();
                        return Some((0, message));
                    }
                    Heard::Spoiled => self.pipe.spoiled_line(),
                    Heard::Nothing => {}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use alloc::collections::VecDeque;
    use alloc::sync::Arc;
    use alloc::vec;
    use std::sync::Mutex;

    type Wire = Arc<Mutex<VecDeque<u8>>>;

    /// Two wires crossed. Each read takes only a few bytes, as a slow reader does.
    struct Crossed {
        to_me: Wire,
        to_them: Wire,
        spoiled: u32,
    }

    impl Pipe for Crossed {
        fn write(&mut self, bytes: &[u8]) {
            self.to_them.lock().unwrap().extend(bytes);
        }

        fn read(&mut self, into: &mut [u8]) -> usize {
            let mut queue = self.to_me.lock().unwrap();
            let size = queue.len().min(into.len()).min(7);
            for (to, from) in into.iter_mut().zip(queue.drain(..size)) {
                *to = from;
            }
            size
        }

        fn spoiled_line(&mut self) {
            self.spoiled += 1;
        }
    }

    fn wired() -> (LineLink<Crossed>, LineLink<Crossed>) {
        let (one, two): (Wire, Wire) = (Wire::default(), Wire::default());
        let left = Crossed { to_me: one.clone(), to_them: two.clone(), spoiled: 0 };
        let right = Crossed { to_me: two, to_them: one, spoiled: 0 };
        (LineLink::new(left), LineLink::new(right))
    }

    /// Two ends with their own Hello clocks and random stays, on one kind of cable.
    fn seconds_to_link(crossed: bool, seed: u32) -> (u64, Hunt, Hunt) {
        let mut state = seed | 1;
        let mut random = move || {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state
        };
        let (mut left, mut right) = (Hunt::new(random()), Hunt::new(random()));
        let offset = u64::from(random() % 1000);
        let mut linked_at = None;
        for now in (10..1_800_000).step_by(10) {
            left.turn(now, random());
            right.turn(now, random());
            if (left.way == right.way) == crossed {
                if now % 1000 == 0 {
                    right.heard(now);
                }
                if (now + offset) / 10 % 100 == 0 {
                    left.heard(now);
                }
            }
            if linked_at.is_none() && left.linked(now) && right.linked(now) {
                linked_at = Some(now);
            }
        }
        (linked_at.expect("the two ends never linked") / 1000, left, right)
    }

    #[test]
    fn either_cable_links_soon_and_then_nothing_swaps_again() {
        for seed in 1..200 {
            for crossed in [false, true] {
                let (seconds, left, right) = seconds_to_link(crossed, seed);
                assert!(seconds <= 40, "took {seconds} s");
                let swaps = left.swaps + right.swaps;
                assert!(swaps <= 2 * seconds as u32 + 2, "{swaps} swaps in {seconds} s");
                assert_eq!((left.way == right.way), crossed);
            }
        }
    }

    #[test]
    fn a_link_that_goes_quiet_starts_looking_again() {
        let mut hunt = Hunt::new(0);
        hunt.turn(1, 0);
        hunt.heard(1_000);
        assert!(!hunt.turn(2_000, 0));
        assert!(hunt.linked(4_000));
        assert!(hunt.linked(1_000 + CLOSE_MS - 1));
        assert!(!hunt.linked(1_000 + CLOSE_MS));
        assert!(hunt.turn(1_000 + CLOSE_MS, 0));
    }

    fn all_of(link: &mut LineLink<Crossed>) -> Vec<Vec<u8>> {
        let mut found = Vec::new();
        while let Some((_, message)) = link.receive() {
            found.push(message);
        }
        found
    }

    #[test]
    fn messages_cross_the_wire_both_ways() {
        let (mut left, mut right) = wired();
        left.send(Target::All, b"first");
        left.send(Target::Peer(0), &[0, 255, 16]);
        right.send(Target::All, b"answer");
        assert_eq!(all_of(&mut right), [b"first".to_vec(), vec![0, 255, 16]]);
        assert_eq!(all_of(&mut left), [b"answer".to_vec()]);
    }

    #[test]
    fn noise_costs_one_line_and_the_next_comes_through() {
        let (mut left, mut right) = wired();
        left.pipe.write(b"\xff\x00garbage\n#zz\n#123\n");
        left.send(Target::All, b"after the noise");
        assert_eq!(all_of(&mut right), [b"after the noise".to_vec()]);
    }

    #[test]
    fn a_line_cut_off_by_a_pulled_wire_is_dropped() {
        let (mut left, mut right) = wired();
        let line = encode(b"half of this never arrives");
        left.pipe.write(&line[..12]);
        left.send(Target::All, b"whole");
        assert_eq!(all_of(&mut right), [b"whole".to_vec()]);
        assert_eq!(right.pipe.spoiled, 1);
    }

    #[test]
    fn one_flipped_bit_spoils_the_line_and_is_counted() {
        let (mut left, mut right) = wired();
        let mut line = encode(b"a block of some length");
        line[9] ^= 0b0000_0001;
        left.pipe.write(&line);
        assert!(all_of(&mut right).is_empty());
        assert_eq!(right.pipe.spoiled, 1);
    }

    #[test]
    fn a_line_with_no_end_cannot_eat_the_memory() {
        let (mut left, mut right) = wired();
        left.pipe.write(&[b'#'; MOST_BYTES * 2]);
        assert!(all_of(&mut right).is_empty());
        assert!(right.lines.line.len() <= MOST_BYTES);
        left.send(Target::All, b"still alive");
        assert_eq!(all_of(&mut right), [b"still alive".to_vec()]);
    }
}
