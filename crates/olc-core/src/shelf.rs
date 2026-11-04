//! Headers that outlive their blocks. A computer has room for all of them, as Bitcoin Core does
//! when it prunes, so it can still serve light nodes after the bodies are gone. A board has no
//! shelf: what it can show stops where its log stops.
use crate::chain::Chain;
use crate::locator::shared_height;
use crate::types::BlockHeader;
use alloc::vec::Vec;
use olc_crypto::Hash;

/// Headers of the best chain by height. A reorganisation puts a new one over the old.
pub trait Shelf: Send {
    fn put(&mut self, height: u64, header: &BlockHeader) -> bool;
    fn get(&self, height: u64) -> Option<BlockHeader>;
}

impl Chain {
    pub(crate) fn shelve(&mut self, height: u64, header: &BlockHeader) {
        if let Some(shelf) = &mut self.shelf {
            // A header lost here costs a peer one answer: the run it asks for stops short.
            let _ = shelf.put(height, header);
        }
    }

    /// The best chain's header at `height`. The shelf only puts a header over another, so after a
    /// shorter branch with more work wins it still holds the old branch above the tip: nothing
    /// above the tip is read from it.
    pub(crate) fn shelved(&self, height: u64) -> Option<BlockHeader> {
        if height > self.height() {
            return None;
        }
        self.shelf.as_ref()?.get(height)
    }

    /// True when headers stay after their blocks go.
    pub fn keeps_every_header(&self) -> bool {
        self.shelf.is_some()
    }

    /// Fills the shelf from what the log holds, for a chain older than its shelf. The tip goes on
    /// last, so a cut partway finds it missing and starts over.
    pub(crate) fn fill_shelf(&mut self) {
        let tip = self.tip_entry();
        if self.shelf.is_none() || tip.height == 0 || self.shelved(tip.height) == Some(tip.header) {
            return;
        }
        let mut cursor = self.parent_of(&tip);
        while let Some(entry) = cursor.filter(|entry| entry.height > 0) {
            if self.shelved(entry.height) == Some(entry.header) {
                break;
            }
            self.shelve(entry.height, &entry.header);
            cursor = self.parent_of(&entry);
        }
        self.shelve(tip.height, &tip.header);
    }

    /// Up to `most` headers of the best chain after the newest one the locator names. `Err` gives
    /// the first height this node can still show, when the asker needs one from below it.
    pub fn headers_after(
        &self,
        locator: &[(u64, Hash)],
        most: usize,
    ) -> Result<Vec<BlockHeader>, u64> {
        let next = shared_height(self, locator) + 1;
        let oldest = self.oldest_held();
        let mut headers: Vec<BlockHeader> = Vec::new();
        let mut height = next;
        while height < oldest && headers.len() < most {
            let joined = |header: &BlockHeader| {
                headers.last().is_none_or(|last| header.previous == last.id())
            };
            let Some(header) = self.shelved(height).filter(joined) else {
                break;
            };
            headers.push(header);
            height += 1;
        }
        if headers.is_empty() && next < oldest {
            return Err(oldest);
        }
        if height >= oldest {
            let left = most - headers.len();
            headers.extend(self.blocks_from(height, left).iter().map(|block| block.header));
        }
        Ok(headers)
    }
}
