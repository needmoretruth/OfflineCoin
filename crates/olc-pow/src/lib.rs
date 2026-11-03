//! Proof of work: find a nonce whose header hash falls below a target.
#![no_std]

pub mod asert;

use olc_core::bits::{self, Wide};
use olc_core::{BlockHeader, Consensus, Hash, PerBlockInput, RetargetInput};

pub struct ProofOfWork;

/// What a header's hash must reach.
enum Goal {
    /// Bitcoin's: the hash, read little-endian, at most the target of the compact bits the
    /// difficulty gives (Bitcoin Core's `CheckProofOfWork`).
    Bitcoin(Wide),
    /// OfflineCoin's first: the hash's first eight bytes at most `u64::MAX / difficulty`, so a
    /// difficulty of N takes about N attempts.
    First(u64),
}

impl Goal {
    fn of(header: &BlockHeader, difficulty: u64) -> Option<Self> {
        if !header.is_bitcoin_shaped() {
            return Some(Self::First(u64::MAX / difficulty.max(1)));
        }
        bits::target_of(bits::bits_of(difficulty)).map(Self::Bitcoin)
    }

    fn met(&self, hash: &Hash) -> bool {
        match self {
            Self::Bitcoin(target) => bits::hash_meets(hash, target),
            Self::First(most) => {
                let mut head = [0u8; 8];
                head.copy_from_slice(&hash[..8]);
                u64::from_be_bytes(head) <= *most
            }
        }
    }
}

impl Consensus for ProofOfWork {
    fn name(&self) -> &'static str {
        "pow"
    }

    /// A header in Bitcoin's shape must also say nothing its 80 bytes leave out, so one id
    /// never stands for two headers.
    fn check_seal(&self, header: &BlockHeader) -> bool {
        header.is_whole() && self.meets(header, header.difficulty)
    }

    fn work(&self, header: &BlockHeader) -> u128 {
        u128::from(header.difficulty)
    }

    /// Bitcoin's retarget: scale by how far off the window was, limited to a factor of four and
    /// never below the coin's floor.
    fn next_difficulty(&self, input: &RetargetInput) -> u64 {
        let expected = u128::from(input.expected_ms.max(1));
        let actual = u128::from(input.actual_ms)
            .clamp(expected / 4, expected * 4)
            .max(1);
        let scaled = u128::from(input.parent_difficulty) * expected / actual;
        scaled.clamp(u128::from(input.min_difficulty.max(1)), u128::from(u64::MAX)) as u64
    }

    /// ASERT anchored on the parent: the spec's formula with the parent as its anchor block, so
    /// its parent's time is the anchor time and one block has passed. Over blocks that stay
    /// between the floor and the top this comes, up to rounding, to what an anchor at the first
    /// block gives. A node needs nothing older than a block's grandparent, and a chain that
    /// sat at the floor through a quiet hour follows new hash power from its next block, where
    /// a fixed anchor would wait for the hour to be made up.
    fn per_block_difficulty(&self, input: &PerBlockInput) -> u64 {
        let gap = i128::from(input.gap_ms);
        let exponent = asert::exponent(gap, 0, input.block_time_ms, input.half_life_ms);
        asert::difficulty(input.parent_difficulty, exponent, input.min_difficulty)
    }

    fn meets(&self, header: &BlockHeader, difficulty: u64) -> bool {
        Goal::of(header, difficulty).is_some_and(|goal| goal.met(&header.id()))
    }

    /// Lays the header out once and changes only its nonce between attempts.
    fn try_meet(&self, header: &mut BlockHeader, difficulty: u64, attempts: u32) -> bool {
        let Some(goal) = Goal::of(header, difficulty) else {
            return false;
        };
        let mut sealing = header.sealing();
        for _ in 0..attempts {
            sealing.set_nonce(header.nonce);
            if goal.met(&sealing.id()) {
                return true;
            }
            header.nonce = header.nonce.wrapping_add(1);
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(difficulty: u64) -> BlockHeader {
        BlockHeader {
            version: 1,
            previous: [0; 32],
            merkle_root: [0; 32],
            time_ms: 1,
            difficulty,
            nonce: 0,
        }
    }

    #[test]
    fn sealing_produces_a_valid_header() {
        let mut candidate = header(500);
        assert!(ProofOfWork.try_seal(&mut candidate, 1_000_000));
        assert!(ProofOfWork.check_seal(&candidate));
    }

    #[test]
    fn retarget_follows_block_time_within_limits() {
        let input = |actual_ms| RetargetInput {
            parent_difficulty: 1_000,
            actual_ms,
            expected_ms: 1_000,
            min_difficulty: 1,
        };
        assert_eq!(ProofOfWork.next_difficulty(&input(500)), 2_000);
        assert_eq!(ProofOfWork.next_difficulty(&input(2_000)), 500);
        assert_eq!(ProofOfWork.next_difficulty(&input(1)), 4_000);
        assert_eq!(ProofOfWork.next_difficulty(&input(1_000_000)), 250);
    }

    #[test]
    fn retarget_stops_at_the_floor() {
        let slow = |parent_difficulty| RetargetInput {
            parent_difficulty,
            actual_ms: 400_000,
            expected_ms: 270_000,
            min_difficulty: 30_000,
        };
        assert_eq!(ProofOfWork.next_difficulty(&slow(30_000)), 30_000);
        assert_eq!(ProofOfWork.next_difficulty(&slow(31_000)), 30_000);
        assert_eq!(ProofOfWork.next_difficulty(&slow(90_000)), 60_750);
        // A first difficulty below the floor comes up to it.
        assert_eq!(ProofOfWork.next_difficulty(&slow(8)), 30_000);
    }
}
