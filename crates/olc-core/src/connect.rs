//! How a block joins: checked, written, followed. A reorganisation walks memory back with undo
//! data and forward with checks, and only a move that worked reaches the log.
use crate::chain::{AddOutcome, Chain, ChainError};
use crate::ledger::{apply_block, spent_by, undo_block, Unspent};
use crate::log::{block_record, read_block_record, Kind, Meta, Read, NOWHERE};
use crate::rules::check_block_body;
use crate::tree::{trim, Entry, Mark, MARK_EVERY};
use crate::types::{merkle_root, Block};
use alloc::collections::BTreeSet;
use alloc::vec::Vec;
use olc_crypto::Hash;

fn meta_of(entry: &Entry) -> Meta {
    Meta {
        parent_at: entry.parent_at,
        height: entry.height,
        total_work: entry.total_work,
        header: entry.header,
    }
}

impl Chain {
    /// Adds a block and follows it if its branch now has the most work.
    pub fn add_block(&mut self, block: Block) -> Result<AddOutcome, ChainError> {
        let id = block.id();
        if self.entries.contains_key(&id) || self.is_buried(&block, &id) {
            return Err(ChainError::AlreadyKnown);
        }
        if !self.plugins.consensus.check_seal(&block.header) {
            return Err(ChainError::BadSeal);
        }
        if block.header.merkle_root != merkle_root(&block.txs) {
            return Err(ChainError::BadMerkleRoot);
        }
        if block.size() as u64 > self.config.rules.max_block_bytes {
            return Err(ChainError::TooLarge);
        }
        let previous = block.header.previous;
        let Some(parent) = self.parent_entry(&block) else {
            self.orphans.insert(id, block);
            return Ok(AddOutcome::Orphan { missing: previous });
        };
        if block.header.time_ms <= parent.header.time_ms {
            return Err(ChainError::BadTime);
        }
        if block.header.difficulty != self.difficulty_after(&parent) {
            return Err(ChainError::BadDifficulty);
        }
        let mut entry = Entry {
            header: block.header,
            height: parent.height + 1,
            total_work: parent.total_work + self.plugins.consensus.work(&block.header),
            at: NOWHERE,
            parent_at: parent.at,
            validated: false,
        };
        if previous == self.tip {
            let rules = &self.config.rules;
            let spent = check_block_body(&block, entry.height, &self.unspent, rules, &self.plugins)?;
            let record = block_record(&meta_of(&entry), &block.txs, Some(&spent));
            entry.at = self.log.append(Kind::Tip, &record).ok_or(ChainError::StoreFailed)?;
            entry.validated = true;
            self.step_forward(entry, &block);
            self.settle();
            return Ok(AddOutcome::NewTip);
        }
        let record = block_record(&meta_of(&entry), &block.txs, None);
        entry.at = self.log.append(Kind::Side, &record).ok_or(ChainError::StoreFailed)?;
        self.entries.insert(id, entry);
        if entry.total_work <= self.total_work() {
            self.settle();
            return Ok(AddOutcome::SideBranch);
        }
        let replaced = self.move_to(id)?;
        self.settle();
        Ok(AddOutcome::Reorganized { replaced })
    }

    /// True for a block of the best chain that memory has already let go of.
    fn is_buried(&self, block: &Block, id: &Hash) -> bool {
        let height = block.txs.first().and_then(|tx| tx.coinbase_height);
        !self.entries.contains_key(&block.header.previous)
            && height.is_some_and(|height| self.id_at(height).as_ref() == Some(id))
    }

    /// The block's parent, in memory or deep on the best chain. A coinbase names its height, which
    /// says where to look, and the id decides whether it was true.
    fn parent_entry(&self, block: &Block) -> Option<Entry> {
        let previous = block.header.previous;
        if let Some(parent) = self.entries.get(&previous) {
            return Some(*parent);
        }
        let claimed = block.txs.first()?.coinbase_height?.checked_sub(1)?;
        let found = self.best_entry_at(claimed)?;
        (found.header.id() == previous).then_some(found)
    }

    pub(crate) fn settle(&mut self) {
        let keep = [self.tip, self.genesis];
        let window = self.window();
        trim(&mut self.entries, window, &keep);
        if self.height().is_multiple_of(16) {
            self.prune();
        }
    }

    /// Moves memory onto a block already in the log.
    pub(crate) fn step_forward(&mut self, entry: Entry, block: &Block) {
        let id = block.id();
        apply_block(block, entry.height, &mut self.unspent);
        self.found.add(block, entry.height);
        self.shelve(entry.height, &block.header);
        if entry.height.is_multiple_of(MARK_EVERY) {
            let at = entry.at;
            let height = entry.height;
            self.marks.push(Mark { height, at, id });
        }
        self.entries.insert(id, entry);
        self.tip = id;
    }

    /// Moves memory back over the tip.
    pub(crate) fn step_back(&mut self, read: &Read) {
        undo_block(&read.block, &read.undo, &mut self.unspent);
        self.found.remove(&read.block);
        self.marks.retain(|mark| mark.height < read.meta.height);
        self.tip = read.block.header.previous;
    }

    pub(crate) fn tip_read(&self, entry: &Entry) -> Option<Read> {
        match self.log.record(entry.at)? {
            (Kind::Tip, bytes) => read_block_record(&bytes),
            _ => None,
        }
    }

    /// Walks from the tip and from `target` to where they meet: what leaves the best chain, tip
    /// first, and what joins it, `target` first.
    fn paths_to(&self, target: &Hash) -> Option<(Vec<Entry>, Vec<Entry>)> {
        let (mut left, mut joined) = (Vec::new(), Vec::new());
        let mut old = self.tip_entry();
        let mut new = *self.entries.get(target)?;
        while old.header != new.header {
            if old.height >= new.height {
                left.push(old);
                old = self.parent_of(&old)?;
            } else {
                joined.push(new);
                new = self.parent_of(&new)?;
            }
        }
        Some((left, joined))
    }

    /// Makes `target` the tip and returns how many blocks left the best chain. A branch that fails
    /// is forgotten and the chain stays where it was, in memory and in the log.
    pub(crate) fn move_to(&mut self, target: Hash) -> Result<u64, ChainError> {
        let (left, joined) = self.paths_to(&target).ok_or(ChainError::ReorgTooDeep)?;
        let fork = self.tip_of_fork(&left, &joined);
        if left.iter().any(|entry| self.tip_read(entry).is_none())
            || joined.iter().any(|entry| self.body_of(entry).is_none())
        {
            return Err(ChainError::ReorgTooDeep);
        }
        for entry in &left {
            if let Some(read) = self.tip_read(entry) {
                self.step_back(&read);
            }
        }
        let mut done: Vec<(Entry, Block, Vec<Unspent>)> = Vec::new();
        for entry in joined.iter().rev() {
            let Some(block) = self.body_of(entry) else {
                continue;
            };
            let spent = if entry.validated {
                Ok(spent_by(&block, entry.height, &self.unspent))
            } else {
                let rules = &self.config.rules;
                check_block_body(&block, entry.height, &self.unspent, rules, &self.plugins)
            };
            match spent {
                Ok(spent) => {
                    self.step_forward(*entry, &block);
                    done.push((*entry, block, spent));
                }
                Err(error) => {
                    self.go_back(&done, &left);
                    self.forget_branch(block.id());
                    return Err(error);
                }
            }
        }
        self.write_move(&fork, &done)?;
        Ok(left.len() as u64)
    }

    fn tip_of_fork(&self, left: &[Entry], joined: &[Entry]) -> Hash {
        let lowest = left.last().or(joined.last());
        lowest.map_or(self.tip, |entry| entry.header.previous)
    }

    /// Puts memory back after a branch failed.
    fn go_back(&mut self, done: &[(Entry, Block, Vec<Unspent>)], left: &[Entry]) {
        for (entry, block, spent) in done.iter().rev() {
            let read = Read {
                meta: meta_of(entry),
                block: block.clone(),
                undo: spent.clone(),
            };
            self.step_back(&read);
        }
        for entry in left.iter().rev() {
            if let Some(read) = self.tip_read(entry) {
                self.step_forward(*entry, &read.block);
            }
        }
    }

    /// Drops a block that failed and everything built on it.
    pub(crate) fn forget_branch(&mut self, bad: Hash) {
        let mut gone = BTreeSet::new();
        gone.insert(bad);
        let mut ids: Vec<(u64, Hash)> = self.entries.iter().map(|(id, e)| (e.height, *id)).collect();
        ids.sort_unstable();
        for (_, id) in ids {
            if self.entries.get(&id).is_some_and(|entry| gone.contains(&entry.header.previous)) {
                gone.insert(id);
            }
        }
        self.entries.retain(|id, _| !gone.contains(id));
    }

    /// Writes a finished move. A cut in the middle leaves a log a boot can finish the move from,
    /// because the branch's blocks are already in it.
    fn write_move(
        &mut self,
        fork: &Hash,
        done: &[(Entry, Block, Vec<Unspent>)],
    ) -> Result<(), ChainError> {
        if !done.is_empty() {
            let rewound = self.log.append(Kind::Rewind, fork.as_slice());
            rewound.ok_or(ChainError::StoreFailed)?;
        }
        let mut parent_at = done.first().map_or(NOWHERE, |(entry, ..)| entry.parent_at);
        for (entry, block, spent) in done {
            let id = block.id();
            let mut moved = *entry;
            moved.parent_at = parent_at;
            let record = block_record(&meta_of(&moved), &block.txs, Some(spent));
            moved.at = self.log.append(Kind::Tip, &record).ok_or(ChainError::StoreFailed)?;
            moved.validated = true;
            parent_at = moved.at;
            self.entries.insert(id, moved);
            if let Some(mark) = self.marks.iter_mut().rev().find(|mark| mark.id == id) {
                mark.at = moved.at;
            }
        }
        Ok(())
    }
}
