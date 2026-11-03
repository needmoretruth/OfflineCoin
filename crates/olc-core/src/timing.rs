//! How a coin's difficulty follows the hash rate, and which times a block may carry. Each coin
//! chooses in its file, and a file from before the choice keeps what every coin had then.
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

/// The three ways a coin's difficulty follows the hash rate.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DifficultyRule {
    /// Bitcoin's (Bitcoin Core `pow.cpp`): every `retarget_interval` blocks the difficulty
    /// follows how long the last window took, at most four times either way.
    #[default]
    Bitcoin,
    /// Bitcoin's test network since BIP 94: Bitcoin's rule, where a block more than two block
    /// times after its parent carries the floor, except the first block of a window, and a
    /// retarget starts from the first block of the window it measures.
    Bip94,
    /// Bitcoin Cash's ASERT (aserti3-2d): every block, the difficulty halves for each half-life
    /// its parent came late and doubles for each half-life it came early. With
    /// `allow_min_difficulty_blocks`, a block late enough may carry the floor, see
    /// [`crate::Chain::floor_after`].
    Asert,
}

/// What a coin says about difficulty and time, in the order the binary form carries it. Older
/// builds fixed the layout of [`crate::Rules`] before these, so they travel after it.
pub type Timing = (DifficultyRule, u64, u64, u64, bool);

/// What every coin had before the choice: Bitcoin's retarget, a block after its parent, no
/// clock limit and no floor for a late block.
pub const LEGACY: Timing = (DifficultyRule::Bitcoin, 0, 0, 0, false);

/// OfflineCoin's half-life in blocks: six, as Bitcoin Cash's test networks, whose hash comes and
/// goes, use one hour of their 10 minute blocks. At 30 seconds a block that is three minutes late
/// halves the difficulty. Counted in blocks, it keeps its meaning when a coin's block time changes.
pub const HALF_LIFE_BLOCKS: u64 = 6;

/// Blocks whose middle time a new block must pass, as in Bitcoin. With 30 second blocks the
/// middle one is about three minutes back.
pub const MEDIAN_TIME_BLOCKS: u64 = 11;

/// How far ahead of a node's clock a block may be: one block time, as Ethereum's miners took 15
/// seconds at their 13. Clocks set over the internet agree within seconds. A late block's floor
/// waits this long as well, so a clock set ahead gains nothing there, and under ASERT a block
/// this far ahead makes the next one at most about 1.12 times easier.
pub const MAX_AHEAD_MS: u64 = 30_000;

/// The most blocks ASERT looks back past late blocks at the floor for the block it anchors on.
/// A node that started from a peer's snapshot holds 64 headers, so every node can look this far.
pub const LONGEST_LATE_RUN: u64 = 32;

/// A clock that reads before 2020 counts from its own start, as a board's does, not the date.
pub const DATED_CLOCK_FROM_MS: u64 = 1_577_836_800_000;

/// The middle of `times`, as Bitcoin's median time past takes it: the later of the two middle
/// ones when there is an even number.
pub fn median(mut times: Vec<u64>) -> u64 {
    times.sort_unstable();
    times.get(times.len() / 2).copied().unwrap_or(0)
}

/// True when `timing` says more than every coin said before the choice, so a coin that never
/// chose keeps the bytes and names it had.
pub fn chosen(timings: &[Timing]) -> bool {
    timings.iter().any(|timing| *timing != LEGACY)
}
