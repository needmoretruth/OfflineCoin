//! What stays in memory about blocks: the headers near each branch's end, and a mark every so many
//! blocks of the best chain. Everything older is found by walking the log.
use crate::types::BlockHeader;
use alloc::collections::{BTreeMap, BTreeSet};
use alloc::vec::Vec;
use olc_crypto::Hash;
use serde::{Deserialize, Serialize};

/// Blocks of the best chain between two marks. A walk to any height is at most this long.
pub const MARK_EVERY: u64 = 256;
/// Headers kept below each branch's end, at the least.
const WINDOW: u64 = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Entry {
    pub header: BlockHeader,
    pub height: u64,
    pub total_work: u128,
    /// Where its record is.
    pub at: u64,
    /// Where its parent's record is.
    pub parent_at: u64,
    /// Set once its transactions have been checked against its history.
    pub validated: bool,
}

/// A block of the best chain that a walk can start from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Mark {
    pub height: u64,
    pub at: u64,
    pub id: Hash,
}

pub(crate) type Entries = BTreeMap<Hash, Entry>;

/// The headers a retarget can reach, at the least.
pub(crate) fn window(retarget_interval: u64) -> u64 {
    retarget_interval.max(WINDOW)
}

/// Keeps `window` headers below every branch end and drops the rest. The newest ends stay when
/// there are too many, because anyone can send blocks and only the best few can matter.
pub(crate) fn trim(entries: &mut Entries, window: u64, pinned: &[Hash]) {
    let limit = (window * 4) as usize;
    if entries.len() < limit {
        return;
    }
    let parents: BTreeSet<Hash> = entries.values().map(|e| e.header.previous).collect();
    let mut ends: Vec<(u64, Hash)> = entries
        .iter()
        .filter(|(id, _)| !parents.contains(*id))
        .map(|(id, entry)| (entry.height, *id))
        .collect();
    ends.sort_unstable();
    let most_ends = (window / 8).max(1) as usize;
    let dropped = ends.len().saturating_sub(most_ends);
    let mut wanted: BTreeSet<Hash> = pinned.iter().copied().collect();
    for (_, end) in ends.into_iter().skip(dropped) {
        let mut cursor = end;
        for _ in 0..window {
            let Some(entry) = entries.get(&cursor) else {
                break;
            };
            wanted.insert(cursor);
            cursor = entry.header.previous;
        }
    }
    entries.retain(|id, _| wanted.contains(id));
}
