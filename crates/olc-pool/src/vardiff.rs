//! Each worker's share difficulty, moved until its shares come about every few seconds, as
//! Stratum pools have done since 2012 (vardiff). A board and a computer a thousand times faster
//! then send about as many shares, so neither floods the pool nor waits minutes to show its work.
use olc_core::bits::{bits_of, difficulty_of};

/// About how often a worker finds a share.
pub const SHARE_EVERY_MS: u64 = 5_000;
/// Shares counted before the difficulty moves, unless the time below runs out first.
const SHARES_TO_JUDGE: u64 = 12;
/// The longest a worker goes before its difficulty is looked at again.
const JUDGE_AFTER_MS: u64 = 4 * SHARE_EVERY_MS;
/// The most the difficulty moves in one step, either way.
const MOST_STEP: u64 = 8;

#[derive(Clone, Debug)]
pub struct Vardiff {
    difficulty: u64,
    since_ms: u64,
    shares: u64,
    /// The difficulty this one replaced and until when shares at it still count.
    before: Option<(u64, u64)>,
}

/// A difficulty a compact target stands for, so the work a share is credited with is the work its
/// target asks.
pub fn rounded(difficulty: u64) -> u64 {
    difficulty_of(bits_of(difficulty.max(1)))
}

impl Vardiff {
    pub fn new(difficulty: u64, now_ms: u64) -> Self {
        Self { difficulty: rounded(difficulty), since_ms: now_ms, shares: 0, before: None }
    }

    pub fn difficulty(&self) -> u64 {
        self.difficulty
    }

    /// The difficulty before the last move, while a miner may still be finishing work at it.
    pub fn before(&self, now_ms: u64) -> Option<u64> {
        self.before.filter(|(_, until)| now_ms < *until).map(|(difficulty, _)| difficulty)
    }

    /// Sets the difficulty outright, as a person who fixed it or a block's own difficulty does.
    pub fn set(&mut self, difficulty: u64, now_ms: u64) -> bool {
        let difficulty = rounded(difficulty);
        if difficulty == self.difficulty {
            return false;
        }
        // A miner takes a new difficulty with its next job, which comes within half a minute.
        self.before = Some((self.difficulty, now_ms + SHARE_EVERY_MS * 6));
        self.difficulty = difficulty;
        self.since_ms = now_ms;
        self.shares = 0;
        true
    }

    pub fn shared(&mut self) {
        self.shares += 1;
    }

    /// Moves the difficulty toward one share every [`SHARE_EVERY_MS`], never past `most`. True
    /// when it moved.
    pub fn judge(&mut self, now_ms: u64, most: u64) -> bool {
        let elapsed = now_ms.saturating_sub(self.since_ms);
        if self.shares < SHARES_TO_JUDGE && elapsed < JUDGE_AFTER_MS {
            return false;
        }
        // Shares that came at this rate would have taken this long each.
        let wanted = u128::from(self.difficulty) * u128::from(self.shares.max(1)) * u128::from(SHARE_EVERY_MS)
            / u128::from(elapsed.max(1));
        let step = u128::from(MOST_STEP);
        let low = u128::from(self.difficulty) / step;
        let high = u128::from(self.difficulty) * step;
        let next = wanted.clamp(low.max(1), high).min(u128::from(most.max(1))) as u64;
        // Within a quarter either way is close enough: moving costs the miner a fresh job.
        let near = next.abs_diff(self.difficulty) <= self.difficulty / 4;
        if near {
            self.since_ms = now_ms;
            self.shares = 0;
            return false;
        }
        self.set(next, now_ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A worker hashing at `rate` a second, its shares found evenly, for `seconds`. Returns the
    /// shares it sent in the last minute.
    fn run(rate: u64, seconds: u64) -> u64 {
        let mut vardiff = Vardiff::new(1_000, 0);
        let mut hashes = 0u128;
        let mut recent = Vec::new();
        for tick in 1..=seconds * 10 {
            let now = tick * 100;
            hashes += u128::from(rate / 10);
            while hashes >= u128::from(vardiff.difficulty()) {
                hashes -= u128::from(vardiff.difficulty());
                vardiff.shared();
                recent.push(now);
            }
            if vardiff.judge(now, u64::MAX) {
                hashes = 0;
            }
        }
        recent.iter().filter(|at| **at + 60_000 > seconds * 1_000).count() as u64
    }

    #[test]
    fn a_slow_and_a_fast_worker_come_to_about_the_same_share_rate() {
        // A board and a computer: a thousand times apart.
        let slow = run(1_000, 600);
        let fast = run(1_000_000, 600);
        let wanted = 60_000 / SHARE_EVERY_MS;
        for (who, shares) in [("slow", slow), ("fast", fast)] {
            assert!(shares >= wanted / 2 && shares <= wanted * 2, "{who}: {shares} shares a minute, want about {wanted}");
        }
    }

    #[test]
    fn the_difficulty_never_passes_the_block_and_the_old_one_counts_a_while() {
        let mut vardiff = Vardiff::new(1_000, 0);
        for _ in 0..100 {
            vardiff.shared();
        }
        assert!(vardiff.judge(1_000, 5_000));
        assert!(vardiff.difficulty() <= rounded(5_000));
        assert_eq!(vardiff.before(1_500), Some(rounded(1_000)));
        assert_eq!(vardiff.before(1_000 + SHARE_EVERY_MS * 6), None);
    }
}
