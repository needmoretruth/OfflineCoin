//! How two nodes find where their chains part. A node names blocks it has, close together near
//! its tip and further apart below, and a peer starts from the newest one it also has.
use crate::chain::Chain;
use crate::types::Block;
use alloc::collections::BTreeSet;
use alloc::vec::Vec;
use olc_crypto::Hash;

const CLOSE_TO_THE_TIP: usize = 6;

impl Chain {
    /// Blocks of the best chain, newest first, always ending with genesis.
    pub fn locator(&self) -> Vec<Hash> {
        let mut ids = Vec::new();
        let mut cursor = self.tip;
        let mut step = 1;
        while let Some(entry) = self.entries.get(&cursor) {
            ids.push(cursor);
            if ids.len() >= CLOSE_TO_THE_TIP {
                step *= 2;
            }
            for _ in 0..step {
                match self.entries.get(&cursor) {
                    Some(entry) if entry.height > 0 => cursor = entry.block.header.previous,
                    _ => break,
                }
            }
            if entry.height == 0 {
                return ids;
            }
        }
        ids
    }

    /// Up to `most` blocks of the best chain, oldest first, after the newest one named.
    pub fn blocks_after(&self, locator: &[Hash], most: usize) -> Vec<&Block> {
        let known: BTreeSet<&Hash> = locator.iter().collect();
        let mut newest_first = Vec::new();
        let mut cursor = self.tip;
        while let Some(entry) = self.entries.get(&cursor) {
            if known.contains(&cursor) || entry.height == 0 {
                break;
            }
            newest_first.push(&entry.block);
            cursor = entry.block.header.previous;
        }
        newest_first.into_iter().rev().take(most).collect()
    }
}
