//! Blocks whose parent has not arrived yet. A node catching up fetches a branch from its tip
//! down, so this holds a long gap, and anyone can send blocks, so it has a limit.
use crate::types::Block;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use olc_crypto::Hash;

/// Several thousand ordinary blocks, and 256 of the largest.
const MAX_BYTES: usize = 1024 * 1024;

#[derive(Default)]
pub struct Orphans {
    blocks: BTreeMap<Hash, (u64, Block)>,
    arrival: BTreeMap<u64, Hash>,
    arrived: u64,
    bytes: usize,
}

impl Orphans {
    pub fn insert(&mut self, id: Hash, block: Block) {
        if self.blocks.contains_key(&id) {
            return;
        }
        self.arrived += 1;
        self.bytes += block.size();
        self.blocks.insert(id, (self.arrived, block));
        self.arrival.insert(self.arrived, id);
        while self.bytes > MAX_BYTES {
            let Some((_, oldest)) = self.arrival.pop_first() else {
                break;
            };
            self.remove(&oldest);
        }
    }

    pub fn contains(&self, id: &Hash) -> bool {
        self.blocks.contains_key(id)
    }

    pub fn get(&self, id: &Hash) -> Option<&Block> {
        self.blocks.get(id).map(|(_, block)| block)
    }

    fn remove(&mut self, id: &Hash) -> Option<Block> {
        let (arrived, block) = self.blocks.remove(id)?;
        self.arrival.remove(&arrived);
        self.bytes -= block.size();
        Some(block)
    }

    /// Takes the blocks waiting for `parent`.
    pub fn take_children(&mut self, parent: &Hash) -> Vec<Block> {
        let ids: Vec<Hash> = self
            .blocks
            .iter()
            .filter(|(_, (_, block))| block.header.previous == *parent)
            .map(|(id, _)| *id)
            .collect();
        ids.iter().filter_map(|id| self.remove(id)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{BlockHeader, Tx, TxOut};
    use alloc::vec;

    fn block(number: u32) -> Block {
        let tx = Tx {
            version: 1,
            inputs: vec![],
            outputs: vec![TxOut {
                value: 1,
                language: 0,
                lock: vec![0; 4_000],
            }],
            coinbase_height: None,
            lock_time: 0,
        };
        let header = BlockHeader {
            version: 1,
            previous: [1; 32],
            merkle_root: [0; 32],
            time_ms: 0,
            difficulty: 1,
            nonce: number,
        };
        Block {
            header,
            txs: vec![tx],
        }
    }

    #[test]
    fn the_oldest_goes_first_and_the_bytes_stay_under_the_limit() {
        let mut orphans = Orphans::default();
        let overflow = (MAX_BYTES / block(0).size()) as u32 + 2;
        for number in 0..overflow {
            let block = block(number);
            orphans.insert(block.id(), block);
        }
        assert!(orphans.bytes <= MAX_BYTES);
        assert_eq!(orphans.blocks.len(), orphans.arrival.len());
        assert!(!orphans.contains(&block(0).id()));
        assert!(orphans.contains(&block(overflow - 1).id()));
    }

    #[test]
    fn children_leave_and_free_their_room() {
        let mut orphans = Orphans::default();
        let one = block(0);
        orphans.insert(one.id(), one.clone());
        assert_eq!(orphans.take_children(&[1; 32]), vec![one.clone()]);
        assert!(!orphans.contains(&one.id()));
        assert_eq!(orphans.bytes, 0);
        assert!(orphans.arrival.is_empty());
    }
}
