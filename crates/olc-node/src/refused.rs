//! Blocks found invalid. Peers on the same genesis with other rules keep announcing them, and
//! without this the node would ask again at every announcement. Nobody is banned for it.
use alloc::collections::{BTreeSet, VecDeque};
use olc_core::{ChainError, Hash};

const MAX_REMEMBERED: usize = 256;

#[derive(Default)]
pub struct Refused {
    ids: BTreeSet<Hash>,
    arrival: VecDeque<Hash>,
}

impl Refused {
    /// A wrong root means these transactions are not the header's, so that block may still come.
    pub fn add_invalid(&mut self, id: Hash, reason: ChainError) {
        if reason != ChainError::BadMerkleRoot {
            self.add(id);
        }
    }

    pub fn add(&mut self, id: Hash) {
        if !self.ids.insert(id) {
            return;
        }
        self.arrival.push_back(id);
        if self.arrival.len() > MAX_REMEMBERED {
            if let Some(oldest) = self.arrival.pop_front() {
                self.ids.remove(&oldest);
            }
        }
    }

    pub fn has(&self, id: &Hash) -> bool {
        self.ids.contains(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_oldest_goes_when_the_set_is_full() {
        let mut refused = Refused::default();
        for number in 0..=MAX_REMEMBERED {
            let mut id = [0; 32];
            id[..8].copy_from_slice(&(number as u64).to_le_bytes());
            refused.add(id);
        }
        assert!(!refused.has(&[0; 32]));
        assert_eq!(refused.ids.len(), MAX_REMEMBERED);
        assert_eq!(refused.arrival.len(), MAX_REMEMBERED);
    }
}
