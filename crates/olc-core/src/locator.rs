//! How two nodes find where their chains part. A node names blocks it has with their heights,
//! close together near its tip and further apart below, and a peer starts from the newest one it
//! also has. Below a node's held blocks only the marks are left, so it names those.
use crate::chain::Chain;
use crate::tree::MARK_EVERY;
use crate::types::Block;
use alloc::vec::Vec;
use olc_crypto::Hash;

const CLOSE_TO_THE_TIP: usize = 6;

/// A node's answer to a locator.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Served {
    /// The next blocks, oldest first. None means the asker is up to date.
    Blocks(Vec<Block>),
    /// The asker needs blocks this node has dropped. `oldest` is the first one still here.
    Pruned { oldest: u64 },
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
        let shared = locator
            .iter()
            .find(|(height, id)| self.id_at(*height).as_ref() == Some(id))
            .map_or(0, |(height, _)| *height);
        let next = shared + 1;
        if next > self.height() {
            return Served::Blocks(Vec::new());
        }
        let oldest = self.oldest_held();
        if next < oldest {
            return Served::Pruned { oldest };
        }
        Served::Blocks(self.blocks_from(next, most))
    }
}
