//! How an input is made: pure noise, or a good message damaged a few times. Noise tests the first
//! bytes a parser reads. Damage gets past those and tests the fields behind them.
use fastrand::Rng;

const SHORTEST_NOISE: usize = 64;
const MIDDLE_NOISE: usize = 1_024;
const LONGEST_NOISE: usize = 8_192;
const MOST_DAMAGES: u8 = 5;

/// Bytes that sit on the edges of lengths and small numbers.
const EDGES: [u8; 8] = [0, 1, 0x7f, 0x80, 0xfe, 0xff, b'\n', b'#'];

fn noise(rng: &mut Rng, into: &mut Vec<u8>) {
    let most = match rng.u8(0..10) {
        0..=6 => SHORTEST_NOISE,
        7..=8 => MIDDLE_NOISE,
        _ => LONGEST_NOISE,
    };
    let size = rng.usize(0..=most);
    into.extend((0..size).map(|_| rng.u8(..)));
}

fn byte_of(rng: &mut Rng) -> u8 {
    if rng.bool() {
        rng.u8(..)
    } else {
        EDGES[rng.usize(0..EDGES.len())]
    }
}

fn damage(rng: &mut Rng, bytes: &mut Vec<u8>, seeds: &[Vec<u8>]) {
    if bytes.is_empty() {
        bytes.push(byte_of(rng));
        return;
    }
    let at = rng.usize(0..bytes.len());
    match rng.u8(0..9) {
        0 => bytes.truncate(at),
        1 => bytes[at] ^= 1 << rng.u8(0..8),
        2 => bytes[at] = byte_of(rng),
        3 => {
            let size = rng.usize(1..=16);
            let fresh: Vec<u8> = (0..size).map(|_| byte_of(rng)).collect();
            bytes.splice(at..at, fresh);
        }
        4 => {
            let end = (at + rng.usize(1..=32)).min(bytes.len());
            bytes.drain(at..end);
        }
        // A length field that promises more than ever comes.
        5 => {
            let end = (at + rng.usize(1..=10)).min(bytes.len());
            bytes[at..end].fill(0xff);
        }
        6 => {
            let other = &seeds[rng.usize(0..seeds.len())];
            bytes.truncate(at);
            let from = rng.usize(0..=other.len());
            bytes.extend_from_slice(&other[from..]);
        }
        7 => {
            let end = (at + rng.usize(1..=64)).min(bytes.len());
            let copy = bytes[at..end].to_vec();
            bytes.splice(at..at, copy);
        }
        _ => {
            let (a, b) = (at, rng.usize(0..bytes.len()));
            bytes.swap(a, b);
        }
    }
}

/// Fills `into` with the next input.
pub fn next_input(rng: &mut Rng, seeds: &[Vec<u8>], into: &mut Vec<u8>) {
    into.clear();
    if seeds.is_empty() || rng.u8(0..5) == 0 {
        noise(rng, into);
        return;
    }
    into.extend_from_slice(&seeds[rng.usize(0..seeds.len())]);
    for _ in 0..rng.u8(0..=MOST_DAMAGES) {
        damage(rng, into, seeds);
    }
}
