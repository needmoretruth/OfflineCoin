//! The chain as a node without blocks knows it: headers only, checked for proof of work and
//! for the difficulty the rules ask, with the most work winning. Nothing here reads a block.
use crate::coin::Rules;
use crate::plugin::{Consensus, RetargetInput};
use crate::types::BlockHeader;
use alloc::vec::Vec;
use olc_crypto::Hash;

/// A branch longer than this is not gathered. At a block in 30 seconds that is nearly three days.
const LONGEST_BRANCH: usize = 8_192;
/// Never trimmed below this, so retargets and reorganisations keep what they read.
const FEWEST_KEPT: usize = LONGEST_BRANCH + 1_024;

/// The difficulty the rules ask after `parent`. `first_time` gives the time of the first block
/// of the window that closes here, and is only read when the difficulty moves.
pub fn difficulty_after(
    rules: &Rules,
    consensus: &dyn Consensus,
    parent: &BlockHeader,
    next_height: u64,
    first_time: impl FnOnce() -> u64,
) -> u64 {
    let interval = rules.retarget_interval;
    if interval == 0 || !next_height.is_multiple_of(interval) || next_height < interval {
        return parent.difficulty;
    }
    consensus.next_difficulty(&RetargetInput {
        parent_difficulty: parent.difficulty,
        actual_ms: parent.time_ms.saturating_sub(first_time()),
        expected_ms: rules.block_time_ms.saturating_mul(interval - 1),
        min_difficulty: rules.floor_at(next_height),
    })
}

/// Why headers were not taken.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeaderRefused {
    /// It joins nothing this node holds.
    NoParent,
    BadSeal,
    BadTime,
    BadDifficulty,
    /// The branch would part further down than this node looks.
    TooLong,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Offered {
    /// All of it was known.
    Nothing,
    /// Valid so far, and not yet more work than the best chain.
    Gathering,
    Extended,
    Reorganized { replaced: u64 },
}

pub struct HeaderChain {
    genesis: Hash,
    /// The height of `headers[0]`.
    base: u64,
    headers: Vec<BlockHeader>,
    work: u128,
    most: usize,
    /// A branch on top of the block at `fork`, until it has the most work.
    side: Vec<BlockHeader>,
    fork: u64,
}

impl HeaderChain {
    /// Holds at most `most` headers, dropping the oldest. A computer can pass `usize::MAX`.
    pub fn new(genesis: BlockHeader, most: usize) -> Self {
        Self {
            genesis: genesis.id(),
            base: 0,
            headers: alloc::vec![genesis],
            work: 0,
            most: most.max(FEWEST_KEPT),
            side: Vec::new(),
            fork: 0,
        }
    }

    pub fn height(&self) -> u64 {
        self.base + self.headers.len() as u64 - 1
    }

    pub fn tip(&self) -> &BlockHeader {
        &self.headers[self.headers.len() - 1]
    }

    pub fn work(&self) -> u128 {
        self.work
    }

    pub fn held(&self) -> usize {
        self.headers.len()
    }

    pub fn header_at(&self, height: u64) -> Option<&BlockHeader> {
        self.headers.get(usize::try_from(height.checked_sub(self.base)?).ok()?)
    }

    fn id_at(&self, height: u64) -> Option<Hash> {
        match height {
            0 => Some(self.genesis),
            _ if height == self.height() => Some(self.tip().id()),
            _ => self.header_at(height + 1).map(|next| next.previous),
        }
    }

    /// The header at `height` on the branch being gathered.
    fn on_side(&self, height: u64) -> Option<&BlockHeader> {
        match height.checked_sub(self.fork + 1) {
            Some(index) => self.side.get(usize::try_from(index).ok()?),
            None => self.header_at(height),
        }
    }

    /// Blocks of the best chain, newest first, ending with genesis. Close together near the tip.
    pub fn locator(&self) -> Vec<(u64, Hash)> {
        let mut named = Vec::new();
        let mut height = self.height();
        let mut step = 1;
        while height > self.base {
            if let Some(id) = self.id_at(height) {
                named.push((height, id));
            }
            if named.len() >= 6 {
                step *= 2;
            }
            height = height.saturating_sub(step);
        }
        named.push((0, self.genesis));
        named
    }

    /// The newest header gathered on a branch, for asking on from there.
    pub fn position(&self) -> Option<(u64, Hash)> {
        let last = self.side.last()?;
        Some((self.fork + self.side.len() as u64, last.id()))
    }

    /// The height of the parent of `first`, found without hashing: every held block but the tip
    /// is named by the one after it.
    fn parent_of(&self, first: &BlockHeader) -> Option<u64> {
        if first.previous == self.tip().id() {
            return Some(self.height());
        }
        let sibling = self.headers.iter().rposition(|held| held.previous == first.previous)?;
        (self.base + sibling as u64).checked_sub(1)
    }

    /// Takes headers that follow each other. They join the best chain, or a branch that takes
    /// over as soon as it holds more work.
    pub fn offer(
        &mut self,
        batch: &[BlockHeader],
        rules: &Rules,
        consensus: &dyn Consensus,
    ) -> Result<Offered, HeaderRefused> {
        let Some(first) = batch.first() else {
            return Ok(Offered::Nothing);
        };
        let mut batch = batch;
        if self.side.last().is_none_or(|last| last.id() != first.previous) {
            self.side.clear();
            self.fork = self.parent_of(first).ok_or(HeaderRefused::NoParent)?;
            while batch.first().is_some_and(|new| Some(new) == self.header_at(self.fork + 1)) {
                self.fork += 1;
                batch = &batch[1..];
            }
        }
        if batch.is_empty() {
            return Ok(Offered::Nothing);
        }
        for header in batch {
            if let Err(refused) = self.follow(header, rules, consensus) {
                self.side.clear();
                return Err(refused);
            }
        }
        Ok(self.settle(consensus))
    }

    fn follow(
        &mut self,
        header: &BlockHeader,
        rules: &Rules,
        consensus: &dyn Consensus,
    ) -> Result<(), HeaderRefused> {
        if self.side.len() >= LONGEST_BRANCH {
            return Err(HeaderRefused::TooLong);
        }
        let height = self.fork + self.side.len() as u64 + 1;
        let parent = *self.on_side(height - 1).ok_or(HeaderRefused::NoParent)?;
        if header.previous != parent.id() {
            return Err(HeaderRefused::NoParent);
        }
        if !consensus.check_seal(header) {
            return Err(HeaderRefused::BadSeal);
        }
        if header.time_ms <= parent.time_ms {
            return Err(HeaderRefused::BadTime);
        }
        let window = height.saturating_sub(rules.retarget_interval);
        let first_time = || self.on_side(window).map_or(parent.time_ms, |first| first.time_ms);
        if header.difficulty != difficulty_after(rules, consensus, &parent, height, first_time) {
            return Err(HeaderRefused::BadDifficulty);
        }
        self.side.push(*header);
        Ok(())
    }

    fn settle(&mut self, consensus: &dyn Consensus) -> Offered {
        let keep = (self.fork + 1 - self.base) as usize;
        let sum = |headers: &[BlockHeader]| {
            headers.iter().fold(0u128, |sum, header| sum + consensus.work(header))
        };
        let shared = self.work - sum(&self.headers[keep..]);
        let work = shared + sum(&self.side);
        if work <= self.work {
            return Offered::Gathering;
        }
        let replaced = self.height() - self.fork;
        self.headers.truncate(keep);
        self.headers.append(&mut self.side);
        self.work = work;
        if self.headers.len() > self.most {
            let extra = self.headers.len() - self.most;
            self.headers.drain(..extra);
            self.base += extra as u64;
        }
        self.fork = self.height();
        match replaced {
            0 => Offered::Extended,
            replaced => Offered::Reorganized { replaced },
        }
    }

    /// Everything needed to come back without asking anyone. Read through [`Self::offer`], so a
    /// damaged file loses what follows the damage and nothing else.
    pub fn saved(&self) -> Option<&[BlockHeader]> {
        (self.base == 0).then_some(&self.headers[1..])
    }
}
