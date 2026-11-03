//! Bitcoin's compact target (`nBits`) beside the difficulty OfflineCoin keeps.
//!
//! A header in Bitcoin's shape carries `bits`, and its double SHA-256, read as a little-endian
//! 256-bit number, must be at most the target `bits` stands for, as in Bitcoin Core's
//! `CheckProofOfWork`. OfflineCoin's difficulty for such a header is that target's work as
//! Bitcoin Core's `GetBlockProof` counts it, `2^256 / (target + 1)`: the hashes a block takes on
//! average. A difficulty maps to the easiest compact target whose work reaches it, so the two
//! always turn into each other the same way and a difficulty only ever rounds up.
use olc_crypto::Hash;

/// A 256-bit number, low word first.
pub type Wide = [u64; 4];

const ALL_ONES: Wide = [u64::MAX; 4];

fn bit_length(value: &Wide) -> u32 {
    let top = value.iter().rposition(|word| *word != 0);
    top.map_or(0, |index| 64 * index as u32 + 64 - value[index].leading_zeros())
}

fn shifted_left(value: &Wide, by: u32) -> Wide {
    let mut out = [0; 4];
    let (words, bits) = ((by / 64) as usize, by % 64);
    for index in (words..4).rev() {
        let low = value[index - words];
        let carry = match (bits, index > words) {
            (0, _) | (_, false) => 0,
            _ => value[index - words - 1] >> (64 - bits),
        };
        out[index] = low << bits | carry;
    }
    out
}

fn shifted_right(value: &Wide, by: u32) -> Wide {
    let mut out = [0; 4];
    let (words, bits) = ((by / 64) as usize, by % 64);
    for index in 0..4usize.saturating_sub(words) {
        let high = value[index + words];
        let carry = match (bits, index + words + 1 < 4) {
            (0, _) | (_, false) => 0,
            _ => value[index + words + 1] << (64 - bits),
        };
        out[index] = high >> bits | carry;
    }
    out
}

fn at_most(left: &Wide, right: &Wide) -> bool {
    left.iter().rev().cmp(right.iter().rev()) != core::cmp::Ordering::Greater
}

fn minus(left: &Wide, right: &Wide) -> Wide {
    let mut out = [0; 4];
    let mut borrow = false;
    for index in 0..4 {
        let (step, first) = left[index].overflowing_sub(right[index]);
        let (step, second) = step.overflowing_sub(u64::from(borrow));
        out[index] = step;
        borrow = first || second;
    }
    out
}

fn plus_one(value: &Wide) -> Option<Wide> {
    let mut out = *value;
    for word in &mut out {
        let (sum, carried) = word.overflowing_add(1);
        *word = sum;
        if !carried {
            return Some(out);
        }
    }
    None
}

/// `value / by` and what is left, for a divisor of one word.
fn divided(value: &Wide, by: u64) -> (Wide, u64) {
    let mut out = [0; 4];
    let mut rest = 0u128;
    for index in (0..4).rev() {
        let part = rest << 64 | u128::from(value[index]);
        out[index] = (part / u128::from(by)) as u64;
        rest = part % u128::from(by);
    }
    (out, rest as u64)
}

/// `value / by`, bit by bit, for a divisor of any size but zero.
fn divided_wide(value: &Wide, by: &Wide) -> Wide {
    let mut quotient = [0; 4];
    let mut rest = [0; 4];
    for bit in (0..256u32).rev() {
        let overflowed = rest[3] >> 63 == 1;
        rest = shifted_left(&rest, 1);
        rest[0] |= value[(bit / 64) as usize] >> (bit % 64) & 1;
        if overflowed || at_most(by, &rest) {
            rest = minus(&rest, by);
            quotient[(bit / 64) as usize] |= 1 << (bit % 64);
        }
    }
    quotient
}

/// The target `bits` stands for, as Bitcoin Core's `SetCompact` reads it. `None` for a negative,
/// zero or overflowing one, which Bitcoin Core's `CheckProofOfWork` refuses.
pub fn target_of(bits: u32) -> Option<Wide> {
    let size = bits >> 24;
    let word = u64::from(bits & 0x007f_ffff);
    if word == 0 || bits & 0x0080_0000 != 0 {
        return None;
    }
    if size <= 3 {
        let small = word >> (8 * (3 - size));
        return (small != 0).then_some([small, 0, 0, 0]);
    }
    let shift = 8 * (size - 3);
    if 64 - word.leading_zeros() + shift > 256 {
        return None;
    }
    Some(shifted_left(&[word, 0, 0, 0], shift))
}

/// `target` in compact form, as Bitcoin Core's `GetCompact` writes it: the leading bytes are
/// kept and the rest dropped, so the target can only get harder.
pub fn compact_of(target: &Wide) -> u32 {
    let mut size = bit_length(target).div_ceil(8);
    let mut word = if size <= 3 {
        target[0] << (8 * (3 - size))
    } else {
        shifted_right(target, 8 * (size - 3))[0]
    };
    if word & 0x0080_0000 != 0 {
        word >>= 8;
        size += 1;
    }
    (word as u32) | size << 24
}

/// The work of a block at `target`, as Bitcoin Core's `GetBlockProof`: `2^256 / (target + 1)`,
/// at most `u64::MAX`.
pub fn work_of(target: &Wide) -> u64 {
    let Some(next) = plus_one(target) else {
        return 1;
    };
    let not: Wide = core::array::from_fn(|index| !target[index]);
    let quotient = divided_wide(&not, &next);
    if quotient[1..].iter().any(|word| *word != 0) {
        return u64::MAX;
    }
    quotient[0].saturating_add(1)
}

/// The easiest compact target whose work reaches `difficulty`.
pub fn bits_of(difficulty: u64) -> u32 {
    let difficulty = difficulty.max(1);
    // The easiest target of that much work is 2^256 / difficulty − 1.
    let (quotient, rest) = divided(&ALL_ONES, difficulty);
    let easiest = match rest == difficulty - 1 {
        true => quotient,
        false => minus(&quotient, &[1, 0, 0, 0]),
    };
    compact_of(&easiest)
}

/// The difficulty `bits` stands for: the work of its target. 0 for bits no block may carry.
pub fn difficulty_of(bits: u32) -> u64 {
    target_of(bits).map_or(0, |target| work_of(&target))
}

/// The difficulty a header in Bitcoin's shape carries for `difficulty`: the work of its compact
/// target, never less than asked.
pub fn rounded(difficulty: u64) -> u64 {
    difficulty_of(bits_of(difficulty))
}

/// True when `hash`, read as Bitcoin reads a block hash (little-endian), is at most `target`.
pub fn hash_meets(hash: &Hash, target: &Wide) -> bool {
    let words: Wide = core::array::from_fn(|index| {
        let mut word = [0; 8];
        word.copy_from_slice(&hash[8 * index..8 * index + 8]);
        u64::from_le_bytes(word)
    });
    at_most(&words, target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bitcoins_own_limit_reads_as_bitcoin_core_reads_it() {
        let limit = target_of(0x1d00_ffff).unwrap();
        assert_eq!(limit, [0, 0, 0, 0xffff_0000]);
        assert_eq!(compact_of(&limit), 0x1d00_ffff);
        // Bitcoin Core's GetBlockProof of the first block: 0x100010001.
        assert_eq!(work_of(&limit), 0x1_0001_0001);
        assert_eq!(target_of(0x0101_0000), Some([1, 0, 0, 0]));
        assert_eq!(target_of(0x0480_0000), None);
        assert_eq!(target_of(0x2300_0001), None);
    }

    #[test]
    fn a_difficulty_rounds_up_once_and_then_stays() {
        let mut difficulty = 1u64;
        while difficulty < u64::MAX / 3 {
            for asked in [difficulty, difficulty + 1, difficulty * 3 / 2] {
                let bits = bits_of(asked);
                let carried = difficulty_of(bits);
                assert!(carried >= asked, "{asked} became {carried}");
                assert_eq!(bits_of(carried), bits, "{asked}");
                assert_eq!(rounded(carried), carried, "{asked}");
            }
            difficulty += difficulty / 3 * 4 + 1;
        }
        assert_eq!(rounded(u64::MAX), difficulty_of(bits_of(u64::MAX)));
    }

    #[test]
    fn rounding_keeps_a_difficulty_within_its_compact_steps() {
        // 23 bits of mantissa keep every difficulty from 2^24 up within 1 part in 2^15.
        for asked in [30_000u64, 1_000_000, 1 << 40, u64::MAX / 7] {
            let carried = rounded(asked);
            assert!(carried - asked <= asked >> 15 || carried - asked <= 1, "{asked} {carried}");
        }
    }

    #[test]
    fn a_hash_meets_a_target_as_a_little_endian_number() {
        let target = target_of(0x1d00_ffff).unwrap();
        let mut hash = [0xff; 32];
        hash[24..].copy_from_slice(&0xfffe_ffffu64.to_le_bytes());
        assert!(hash_meets(&hash, &target));
        hash[24..].copy_from_slice(&0x1_0000_0000u64.to_le_bytes());
        assert!(!hash_meets(&hash, &target));
    }
}
