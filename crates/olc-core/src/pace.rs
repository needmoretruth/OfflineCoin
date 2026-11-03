//! What a block's header must say, read from the blocks below it: its difficulty under the
//! coin's rule, the earliest time it may carry and, on a coin with Bitcoin's header, its
//! version. A node with blocks and a node with headers only both ask here, so they agree.
use crate::bits;
use crate::coin::{CoinConfig, Rules};
use crate::headers::HeaderRefused;
use crate::plugin::{Consensus, PerBlockInput, RetargetInput};
use crate::timing::{median, DifficultyRule};
use crate::types::{BlockHeader, ROLLED_BITS};
use alloc::vec::Vec;

/// The header at a height up to the parent's, on the branch the new block extends.
pub type Below<'a> = &'a dyn Fn(u64) -> Option<BlockHeader>;

/// The difficulty a block at `next_height` on `parent` must carry, if its time is `time_ms`.
/// A fork's first block asks what the rules before it ask, raised to its floor; on a coin from
/// before Bitcoin's header it adds its mark, so the one number it cannot be is the old one.
pub fn difficulty_after(
    coin: &CoinConfig,
    consensus: &dyn Consensus,
    parent: &BlockHeader,
    next_height: u64,
    time_ms: u64,
    below: Below,
) -> u64 {
    let fork = coin.fork_starting_at(next_height);
    let rules = coin.rules_at(next_height - u64::from(fork.is_some()));
    let asked = asked(coin, rules, consensus, parent, next_height, time_ms, below);
    let floored = fork.map_or(asked, |fork| asked.max(fork.rules.floor_at(next_height)));
    if coin.bitcoin_header {
        return bits::rounded(floored);
    }
    match fork {
        None => asked,
        Some(fork) => {
            let mark = fork.mark();
            floored.checked_add(mark).unwrap_or(u64::MAX - mark)
        }
    }
}

/// The difficulty `rules` ask, before a fork's floor and mark.
fn asked(
    coin: &CoinConfig,
    rules: &Rules,
    consensus: &dyn Consensus,
    parent: &BlockHeader,
    next_height: u64,
    time_ms: u64,
    below: Below,
) -> u64 {
    let interval = rules.retarget_interval;
    let floor = rules.floor_at(next_height);
    if interval == 0 {
        return parent.difficulty;
    }
    if rules.difficulty_rule == DifficultyRule::Asert {
        let Some(grandparent) = next_height.checked_sub(2).and_then(below) else {
            return parent.difficulty;
        };
        let gap = i128::from(parent.time_ms) - i128::from(grandparent.time_ms);
        return consensus.per_block_difficulty(&PerBlockInput {
            parent_difficulty: parent.difficulty,
            gap_ms: i64::try_from(gap).unwrap_or(if gap < 0 { i64::MIN } else { i64::MAX }),
            block_time_ms: rules.block_time_ms,
            half_life_ms: rules.half_life_ms,
            min_difficulty: floor,
        });
    }
    let bip94 = rules.difficulty_rule == DifficultyRule::Bip94;
    if !next_height.is_multiple_of(interval) || next_height < interval {
        if !bip94 {
            return parent.difficulty;
        }
        // Bitcoin Core's test network rule: a block more than two block times after its parent
        // carries the floor, and any other the last difficulty that is not such a block's.
        if time_ms > parent.time_ms.saturating_add(rules.block_time_ms.saturating_mul(2)) {
            return floor;
        }
        let carried = shaped(coin, floor);
        let (mut height, mut header) = (next_height - 1, *parent);
        while height > 0 && !height.is_multiple_of(interval) && header.difficulty == carried {
            height -= 1;
            match below(height) {
                Some(older) => header = older,
                None => break,
            }
        }
        return header.difficulty;
    }
    // The window runs from the block `interval` back, as Bitcoin Core's `pindexFirst`.
    let first = below(next_height - interval);
    // BIP 94 starts from that first block, which never carries the floor's exception.
    let base = match bip94 {
        true => first.map_or(parent.difficulty, |first| first.difficulty),
        false => parent.difficulty,
    };
    consensus.next_difficulty(&RetargetInput {
        parent_difficulty: base,
        actual_ms: parent.time_ms.saturating_sub(first.map_or(parent.time_ms, |first| first.time_ms)),
        expected_ms: rules.block_time_ms.saturating_mul(interval - 1),
        min_difficulty: floor,
    })
}

/// `difficulty` as a header of this coin carries it.
fn shaped(coin: &CoinConfig, difficulty: u64) -> u64 {
    match coin.bitcoin_header {
        true => bits::rounded(difficulty),
        false => difficulty,
    }
}

/// The earliest time a block at `next_height` on `parent` may carry: after the middle time of
/// the blocks the rules count (Bitcoin's median time past), or after its parent where they count
/// none, as every coin did before. Under BIP 94 the first block of a window may also be at most
/// one block time before its parent. Whole seconds on a coin with Bitcoin's header.
pub fn earliest_time(coin: &CoinConfig, parent: &BlockHeader, next_height: u64, below: Below) -> u64 {
    let rules = coin.rules_at(next_height);
    let mut earliest = match rules.median_time_blocks {
        0 => parent.time_ms.saturating_add(1),
        count => {
            let heights = next_height.saturating_sub(count)..next_height;
            let times: Vec<u64> = heights.filter_map(below).map(|header| header.time_ms).collect();
            median(times).saturating_add(1)
        }
    };
    let interval = rules.retarget_interval;
    if rules.difficulty_rule == DifficultyRule::Bip94 && interval > 0 && next_height.is_multiple_of(interval) {
        earliest = earliest.max(parent.time_ms.saturating_sub(rules.block_time_ms));
    }
    match coin.bitcoin_header {
        true => earliest.div_ceil(1_000).saturating_mul(1_000),
        false => earliest,
    }
}

/// Checks what a header at `height` on `parent` says against the rules, all but its seal.
pub fn check(
    coin: &CoinConfig,
    consensus: &dyn Consensus,
    header: &BlockHeader,
    parent: &BlockHeader,
    height: u64,
    below: Below,
) -> Result<(), HeaderRefused> {
    if header.time_ms < earliest_time(coin, parent, height, below) {
        return Err(HeaderRefused::BadTime);
    }
    // The version names the fork a block is on, as the difficulty does on a coin from before.
    // Miners may roll the bits BIP 320 gives them.
    if coin.bitcoin_header && header.version & !ROLLED_BITS != coin.header_version_at(height) {
        return Err(HeaderRefused::BadDifficulty);
    }
    if header.difficulty != difficulty_after(coin, consensus, parent, height, header.time_ms, below) {
        return Err(HeaderRefused::BadDifficulty);
    }
    Ok(())
}
