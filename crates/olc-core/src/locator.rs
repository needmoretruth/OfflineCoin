//! How two nodes find where their chains part. A node names blocks it has with their heights,
//! close together near its tip and further apart below, and a peer starts from the newest one it
//! also has. Below a node's held blocks only the marks are left, so it names those.
use crate::chain::Chain;
use crate::tree::MARK_EVERY;
use crate::types::Block;
use alloc::vec::Vec;
use olc_crypto::Hash;

const CLOSE_TO_THE_TIP: usize = 6;
/// Past this a list names no more blocks. A real one doubles its steps, so it stays well below
/// this on any chain, and each name costs the one answering a look into its store.
pub const MOST_NAMED: usize = 64;

/// A node's answer to a locator.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Served {
    /// The next blocks, oldest first. None means the asker is up to date.
    Blocks(Vec<Block>),
    /// The asker needs blocks this node has dropped. `oldest` is the first one still here.
    Pruned { oldest: u64 },
}

/// The newest block the locator names that is on this chain.
pub(crate) fn shared_height(chain: &Chain, locator: &[(u64, Hash)]) -> u64 {
    locator
        .iter()
        .take(MOST_NAMED)
        .find(|(height, id)| chain.id_at(*height).as_ref() == Some(id))
        .map_or(0, |(height, _)| *height)
}

impl Chain {
    /// Blocks of the best chain, newest first, always ending with genesis.
    pub fn locator(&self) -> Vec<(u64, Hash)> {
        let mut ids = Vec::new();
        let oldest = self.oldest_held();
        let mut height = self.height();
        let mut step = 1;
        while height > 0 {
            let id = match height < oldest {
                true => height - height % MARK_EVERY,
                false => height,
            };
            if let Some(id_there) = self.id_at(id).filter(|_| id > 0) {
                ids.push((id, id_there));
            }
            height = id;
            if ids.len() >= CLOSE_TO_THE_TIP {
                step *= 2;
            }
            height = height.saturating_sub(step);
        }
        ids.push((0, self.genesis));
        ids
    }

    /// Answers a locator with up to `most` blocks of the best chain after the newest one named.
    pub fn blocks_after(&self, locator: &[(u64, Hash)], most: usize) -> Served {
        let next = shared_height(self, locator) + 1;
        if next > self.height() {
            return Served::Blocks(Vec::new());
        }
        let oldest = self.oldest_held();
        if next < oldest {
            return Served::Pruned { oldest };
        }
        Served::Blocks(self.blocks_from(next, most))
    }

    /// The same for a peer that named its blocks without heights, as older builds do. Only a walk
    /// down the held headers can place those, and genesis says nothing about how far it got. The
    /// caller rations this: it reads the store once for each block walked.
    pub fn blocks_after_ids(&self, ids: &[Hash], most: usize) -> Served {
        let shared = self.newest_named(ids).unwrap_or((0, self.genesis));
        self.blocks_after(&[shared], most)
    }

    fn newest_named(&self, ids: &[Hash]) -> Option<(u64, Hash)> {
        let ids = &ids[..ids.len().min(MOST_NAMED)];
        if ids.iter().all(|id| *id == self.genesis) {
            return None;
        }
        let oldest = self.oldest_held().max(1);
        let mut cursor = self.tip_entry();
        while cursor.height >= oldest {
            let id = cursor.header.id();
            if ids.contains(&id) {
                return Some((cursor.height, id));
            }
            cursor = self.parent_of(&cursor)?;
        }
        None
    }
}
