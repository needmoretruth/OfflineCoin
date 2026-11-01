//! Proof of work: find a nonce whose header hash falls below a target.
#![no_std]

use olc_core::{BlockHeader, Consensus, RetargetInput};

pub struct ProofOfWork;

impl ProofOfWork {
    /// A hash passes when its first eight bytes are at most `u64::MAX / difficulty`,
    /// so a difficulty of N takes about N attempts.
    fn meets_target(header_hash: &[u8; 32], difficulty: u64) -> bool {
        let mut head = [0u8; 8];
        head.copy_from_slice(&header_hash[..8]);
        u64::from_be_bytes(head) <= u64::MAX / difficulty.max(1)
    }
}

impl Consensus for ProofOfWork {
    fn name(&self) -> &'static str {
        "pow"
    }

    fn check_seal(&self, header: &BlockHeader) -> bool {
        Self::meets_target(&header.id(), header.difficulty)
    }

    fn work(&self, header: &BlockHeader) -> u128 {
        u128::from(header.difficulty)
    }

    /// Bitcoin's retarget: scale by how far off the window was, limited to a factor of four.
    fn next_difficulty(&self, input: &RetargetInput) -> u64 {
        let expected = u128::from(input.expected_ms.max(1));
        let actual = u128::from(input.actual_ms)
            .clamp(expected / 4, expected * 4)
            .max(1);
        let scaled = u128::from(input.parent_difficulty) * expected / actual;
        scaled.clamp(1, u128::from(u64::MAX)) as u64
    }

    fn meets(&self, header: &BlockHeader, difficulty: u64) -> bool {
        Self::meets_target(&header.id(), difficulty)
    }

    fn try_meet(&self, header: &mut BlockHeader, difficulty: u64, attempts: u32) -> bool {
        for _ in 0..attempts {
            if self.meets(header, difficulty) {
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
        };
        assert_eq!(ProofOfWork.next_difficulty(&input(500)), 2_000);
        assert_eq!(ProofOfWork.next_difficulty(&input(2_000)), 500);
        assert_eq!(ProofOfWork.next_difficulty(&input(1)), 4_000);
        assert_eq!(ProofOfWork.next_difficulty(&input(1_000_000)), 250);
    }
}
