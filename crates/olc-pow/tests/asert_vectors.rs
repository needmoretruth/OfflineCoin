//! Bitcoin Cash's published aserti3-2d test vectors, run two ways (see `asert/NOTICE`).
//!
//! 1. Exactly: each row's 256-bit target, from the spec's steps with OfflineCoin's exponent and
//!    cubic, must give the row's compact `nBits`. Seconds become milliseconds (× 1,000 for every
//!    time, the 600 second block and the two day half-life), which leaves each exponent as it is.
//! 2. Converted: OfflineCoin's difficulty is the target's inverse, so a target `T` stands for the
//!    difficulty `SCALE × limit / T`, where `limit` is the spec's easiest target and `SCALE` the
//!    floor. From the anchor's difficulty, OfflineCoin's own step must land within 1 part in 2,000
//!    of each row: the cubic is off by at most about 0.01 % each way and `nBits` keeps 15 bits.
//!    The 5,144 rows whose difficulty fits in 64 bits are checked; the rest are left to the exact
//!    run.
use olc_pow::asert;

/// A number of 384 bits, low word first: wide enough for a target times the factor, and for a
/// difficulty times a target.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Wide([u64; 6]);

impl Wide {
    fn from(value: u128) -> Self {
        let mut words = [0; 6];
        words[0] = value as u64;
        words[1] = (value >> 64) as u64;
        Self(words)
    }

    fn bits(&self) -> u32 {
        let top = self.0.iter().rposition(|word| *word != 0);
        top.map_or(0, |index| 64 * index as u32 + 64 - self.0[index].leading_zeros())
    }

    fn times(&self, by: u128) -> Self {
        let (low, high) = (by as u64, (by >> 64) as u64);
        let mut sum = self.times_word(low);
        if high != 0 {
            let upper = self.times_word(high).shifted_left(64);
            sum = sum.plus(&upper);
        }
        sum
    }

    fn times_word(&self, by: u64) -> Self {
        let mut out = [0; 6];
        let mut carry = 0u128;
        for (index, word) in self.0.iter().enumerate() {
            let product = u128::from(*word) * u128::from(by) + carry;
            out[index] = product as u64;
            carry = product >> 64;
        }
        assert_eq!(carry, 0, "overflow");
        Self(out)
    }

    fn plus(&self, other: &Self) -> Self {
        let mut out = [0; 6];
        let mut carry = 0u128;
        for index in 0..6 {
            let sum = u128::from(self.0[index]) + u128::from(other.0[index]) + carry;
            out[index] = sum as u64;
            carry = sum >> 64;
        }
        Self(out)
    }

    fn shifted_left(&self, by: u32) -> Self {
        assert!(self.bits() + by <= 384, "overflow");
        let mut out = [0u64; 6];
        for bit in 0..384u32 {
            if bit >= by && self.bit(bit - by) {
                out[(bit / 64) as usize] |= 1 << (bit % 64);
            }
        }
        Self(out)
    }

    fn shifted_right(&self, by: u32) -> Self {
        let mut out = [0u64; 6];
        for bit in 0..384u32 {
            if bit + by < 384 && self.bit(bit + by) {
                out[(bit / 64) as usize] |= 1 << (bit % 64);
            }
        }
        Self(out)
    }

    fn bit(&self, bit: u32) -> bool {
        self.0[(bit / 64) as usize] >> (bit % 64) & 1 == 1
    }

    fn low(&self) -> u64 {
        self.0[0]
    }
}

impl PartialOrd for Wide {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.0.iter().rev().cmp(other.0.iter().rev()))
    }
}

/// Bitcoin's `SetCompact`.
fn target(bits: u32) -> Wide {
    let size = bits >> 24;
    let word = u128::from(bits & 0x007f_ffff);
    if size <= 3 {
        Wide::from(word >> (8 * (3 - size)))
    } else {
        Wide::from(word).shifted_left(8 * (size - 3))
    }
}

/// Bitcoin's `GetCompact`.
fn compact(target: &Wide) -> u32 {
    let mut size = target.bits().div_ceil(8);
    let mut word = if size <= 3 {
        target.low() << (8 * (3 - size))
    } else {
        target.shifted_right(8 * (size - 3)).low()
    };
    if word & 0x0080_0000 != 0 {
        word >>= 8;
        size += 1;
    }
    (word as u32) | (size << 24)
}

const LIMIT_BITS: u32 = 0x1d00_ffff;
const BLOCK_MS: u64 = 600_000;
const HALF_LIFE_MS: u64 = 172_800_000;
/// The floor a converted difficulty starts from, the inverse of the spec's easiest target.
const SCALE: u128 = 1 << 20;

/// The spec's `next_target_aserti3_2d` on 256-bit targets, with OfflineCoin's exponent and cubic.
fn spec_target(anchor_bits: u32, exponent: i128) -> u32 {
    let limit = target(LIMIT_BITS);
    let (shifts, factor) = asert::power_of_two(exponent);
    let product = target(anchor_bits).times(factor);
    let shifted = if shifts >= 0 {
        // The spec's note 6: past 2^16 times the limit, the answer is the limit.
        let left = u32::try_from(shifts).unwrap_or(u32::MAX);
        if product.bits().saturating_add(left) > limit.bits() + 18 {
            return LIMIT_BITS;
        }
        product.shifted_left(left)
    } else {
        product.shifted_right(u32::try_from(-shifts).unwrap_or(u32::MAX).min(384))
    };
    let next = shifted.shifted_right(16);
    if next == Wide::from(0) {
        return compact(&Wide::from(1));
    }
    if next > limit {
        return LIMIT_BITS;
    }
    compact(&next)
}

/// The difficulty `target` stands for, when it fits in 64 bits: the limit's mantissa and size
/// against the target's, so no wide division is needed.
fn converted(bits: u32) -> Option<u64> {
    let (size, mantissa) = (bits >> 24, u128::from(bits & 0x007f_ffff));
    let gap = 8 * 0x1du32.checked_sub(size)?;
    let base = SCALE * 0xffff;
    if base.leading_zeros() <= gap || mantissa == 0 {
        return None;
    }
    u64::try_from((base << gap) / mantissa).ok()
}

struct Run {
    name: String,
    anchor_height: i128,
    anchor_parent_time: i128,
    anchor_bits: u32,
    /// Height, time and the row's `nBits`.
    rows: Vec<(i128, i128, u32)>,
}

fn header_value(text: &str, key: &str) -> String {
    let line = text.lines().find(|line| line.contains(key)).unwrap_or_else(|| panic!("no {key}"));
    line.rsplit(':').next().unwrap_or_default().trim().to_string()
}

fn runs() -> Vec<Run> {
    let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/asert");
    (1..=12)
        .map(|number| {
            let name = format!("run{number:02}");
            let text = std::fs::read_to_string(folder.join(&name)).unwrap();
            let bits = header_value(&text, "anchor nBits");
            let parent = text.lines().find(|line| line.contains("anchor ancestor time") || line.contains("anchor parent time"));
            let parent = parent.unwrap().rsplit(':').next().unwrap().trim().parse().unwrap();
            let rows = text
                .lines()
                .filter(|line| !line.starts_with('#') && !line.trim().is_empty())
                .map(|line| {
                    let fields: Vec<&str> = line.split_whitespace().collect();
                    let row_bits = u32::from_str_radix(fields[3].trim_start_matches("0x"), 16).unwrap();
                    (fields[1].parse().unwrap(), fields[2].parse().unwrap(), row_bits)
                })
                .collect();
            Run {
                name,
                anchor_height: header_value(&text, "anchor height").parse().unwrap(),
                anchor_parent_time: parent,
                anchor_bits: u32::from_str_radix(bits.trim_start_matches("0x"), 16).unwrap(),
                rows,
            }
        })
        .collect()
}

fn exponent_of(run: &Run, height: i128, time: i128) -> i128 {
    let time_delta_ms = (time - run.anchor_parent_time) * 1_000;
    asert::exponent(time_delta_ms, height - run.anchor_height, BLOCK_MS, HALF_LIFE_MS)
}

#[test]
fn every_published_row_comes_out_exactly() {
    let mut rows = 0;
    for run in runs() {
        assert!(!run.rows.is_empty(), "{}", run.name);
        for (height, time, bits) in &run.rows {
            let got = spec_target(run.anchor_bits, exponent_of(&run, *height, *time));
            assert_eq!(got, *bits, "{} height {height}", run.name);
            rows += 1;
        }
    }
    assert_eq!(rows, 14_000);
}

#[test]
fn every_row_in_difficulty_lands_within_its_rounding() {
    let (mut checked, mut worst) = (0, 0.0f64);
    for run in runs() {
        let Some(anchor) = converted(run.anchor_bits) else { continue };
        for (height, time, bits) in &run.rows {
            let Some(expected) = converted(*bits) else { continue };
            let floor = u64::try_from(SCALE).unwrap();
            let got = asert::difficulty(anchor, exponent_of(&run, *height, *time), floor);
            let off = (got as f64 / expected as f64 - 1.0).abs();
            assert!(off < 1.0 / 2_000.0, "{} height {height}: {got} for {expected}", run.name);
            worst = worst.max(off);
            checked += 1;
        }
    }
    eprintln!("{checked} rows in difficulty, worst off by {:.5} %", worst * 100.0);
    assert_eq!(checked, 5_144, "rows whose difficulty fits in 64 bits");
}
