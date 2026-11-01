//! Follows the chain with the most work and tracks unspent outputs.
//!
//! Memory holds the unspent set and the headers near the tip. Block bodies live in the [`Log`],
//! with what undoing each one takes, so a reorganisation walks back instead of starting over and a
//! node can drop its oldest bodies without changing what it validates or mines.
use crate::coin::{CoinConfig, Rules};
pub use crate::ledger::Unspent;
use crate::ledger::{Found, Overlay, UnspentSet};
use crate::log::{read_block_record, Kind, Log, NOWHERE};
use crate::medium::{Keep, Medium, SECTOR};
use crate::orphans::Orphans;
use crate::plugin::{Plugins, RetargetInput};
use crate::rules;
use crate::state::{Opened, Saving};
use crate::tree::{window, Entries, Entry, Mark};
use crate::types::{merkle_root, Block, BlockHeader, OutPoint, Tx, TxOut};
use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;
use olc_crypto::{Address, Hash};

/// A node keeps at least this many recent blocks, as Bitcoin Core's `-prune` does.
pub const FEWEST_RECENT: u64 = 288;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChainError {
    AlreadyKnown,
    BadSeal,
    BadDifficulty,
    BadMerkleRoot,
    BadTime,
    TooLarge,
    NoCoinbase,
    BadCoinbase,
    MissingInput,
    DuplicateInput,
    ImmatureCoinbase,
    BadLock,
    UnknownLanguage,
    Overspend,
    FeeTooLow,
    /// The other branch parts from this one below the blocks still held here.
    ReorgTooDeep,
    /// The block could not be written, so the node did not take it.
    StoreFailed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AddOutcome {
    /// The block extended the best chain.
    NewTip,
    /// The block moved the best chain to another branch.
    Reorganized { replaced: u64 },
    /// The block is stored on a branch with less work.
    SideBranch,
    /// The parent is unknown. The caller should fetch it.
    Orphan { missing: Hash },
}

/// How much of the chain a node holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Held {
    /// The oldest block of the best chain whose body is still here.
    pub oldest: u64,
    pub bytes: u64,
    /// Bytes the store can hold, when it has a limit.
    pub capacity: Option<u64>,
    /// How many recent blocks are kept. `None` keeps all that fit.
    pub recent: Option<u64>,
    /// Blocks that fit, from the size of those held now.
    pub fits: Option<u64>,
}

pub struct Chain {
    pub(crate) config: CoinConfig,
    pub(crate) plugins: Plugins,
    pub(crate) genesis: Hash,
    pub(crate) genesis_block: Block,
    pub(crate) entries: Entries,
    /// A block of the best chain every so many heights, lowest first.
    pub(crate) marks: Vec<Mark>,
    pub(crate) orphans: Orphans,
    pub(crate) tip: Hash,
    pub(crate) unspent: UnspentSet,
    pub(crate) found: Found,
    pub(crate) log: Log,
    /// The two places for the saved state.
    pub(crate) places: [Box<dyn Medium>; 2],
    pub(crate) recent: Option<u64>,
    /// The peer whose unspent set this chain started from, if any.
    pub(crate) assumed_from: Option<u32>,
    pub(crate) opened: Opened,
    /// Where the last whole saved state stops. Records after it are all a boot has to read.
    pub(crate) saved_head: u64,
    pub(crate) saved_seq: u64,
    pub(crate) saved_height: u64,
    pub(crate) saving: Option<Saving>,
    pub(crate) state_too_big: bool,
}

impl Chain {
    /// A chain held in memory only.
    pub fn new(config: CoinConfig, plugins: Plugins) -> Self {
        Self::open(config, plugins, Keep::in_memory())
    }

    pub fn config(&self) -> &CoinConfig {
        &self.config
    }

    pub fn rules(&self) -> &Rules {
        &self.config.rules
    }

    pub fn plugins(&self) -> &Plugins {
        &self.plugins
    }

    pub fn genesis_id(&self) -> Hash {
        self.genesis
    }

    pub fn tip(&self) -> Hash {
        self.tip
    }

    pub(crate) fn tip_entry(&self) -> Entry {
        self.entries[&self.tip]
    }

    pub fn tip_header(&self) -> BlockHeader {
        self.tip_entry().header
    }

    pub fn height(&self) -> u64 {
        self.tip_entry().height
    }

    pub fn total_work(&self) -> u128 {
        self.tip_entry().total_work
    }

    pub fn contains(&self, id: &Hash) -> bool {
        self.entries.contains_key(id) || self.orphans.contains(id)
    }

    /// The peer this chain's unspent set came from, when it did not come from blocks.
    pub fn assumed_from(&self) -> Option<u32> {
        self.assumed_from
    }

    /// What happened when this chain opened its store.
    pub fn opened(&self) -> Opened {
        self.opened
    }

    /// True when the state has grown too big for its place, so boots take longer.
    pub fn state_too_big(&self) -> bool {
        self.state_too_big
    }

    pub(crate) fn window(&self) -> u64 {
        window(self.config.rules.retarget_interval)
    }

    /// The block before `entry`, from memory or else from the log.
    pub(crate) fn parent_of(&self, entry: &Entry) -> Option<Entry> {
        if let Some(parent) = self.entries.get(&entry.header.previous) {
            return Some(*parent);
        }
        if entry.parent_at == NOWHERE {
            return None;
        }
        let meta = self.log.meta(entry.parent_at)?;
        Some(Entry {
            header: meta.header,
            height: meta.height,
            total_work: meta.total_work,
            at: entry.parent_at,
            parent_at: meta.parent_at,
            validated: true,
        })
    }

    /// The best chain's block at `height`, when the walk to it stays on held blocks.
    pub(crate) fn best_entry_at(&self, height: u64) -> Option<Entry> {
        let tip = self.tip_entry();
        if height > tip.height {
            return None;
        }
        let next = self.marks.partition_point(|mark| mark.height < height);
        let start = self.marks.get(next).and_then(|mark| {
            let known = self.entries.get(&mark.id).copied();
            known.or_else(|| {
                let meta = self.log.meta(mark.at)?;
                Some(Entry {
                    header: meta.header,
                    height: meta.height,
                    total_work: meta.total_work,
                    at: mark.at,
                    parent_at: meta.parent_at,
                    validated: true,
                })
            })
        });
        let mut cursor = start.unwrap_or(tip);
        while cursor.height > height {
            cursor = self.parent_of(&cursor)?;
        }
        Some(cursor)
    }

    /// The id of the best chain's block at `height`. Very old blocks may have gone.
    /// The height of a block this node still has in memory.
    pub fn height_of_block(&self, id: &Hash) -> Option<u64> {
        self.entries.get(id).map(|entry| entry.height)
    }

    pub fn id_at(&self, height: u64) -> Option<Hash> {
        if height == 0 {
            return Some(self.genesis);
        }
        let next = self.marks.partition_point(|mark| mark.height < height);
        match self.marks.get(next) {
            Some(mark) if mark.height == height => Some(mark.id),
            _ => self.best_entry_at(height).map(|entry| entry.header.id()),
        }
    }

    pub(crate) fn body_of(&self, entry: &Entry) -> Option<Block> {
        if entry.height == 0 {
            return Some(self.genesis_block.clone());
        }
        let (kind, bytes) = self.log.record(entry.at)?;
        (kind != Kind::Rewind).then(|| read_block_record(&bytes).map(|read| read.block))?
    }

    /// A block held here, by id. Only recent blocks can be found this way: use
    /// [`Chain::blocks_from`] for the rest of the best chain.
    pub fn block(&self, id: &Hash) -> Option<Block> {
        let entry = self.entries.get(id);
        entry
            .and_then(|entry| self.body_of(entry))
            .or_else(|| self.orphans.get(id).cloned())
    }

    /// Up to `most` blocks of the best chain from `first` upward, in order, stopping where the
    /// held blocks stop.
    pub fn blocks_from(&self, first: u64, most: usize) -> Vec<Block> {
        let height = self.height();
        if most == 0 || first > height {
            return Vec::new();
        }
        let last = height.min(first.saturating_add(most as u64 - 1));
        let mut path = Vec::new();
        let mut cursor = self.best_entry_at(last);
        while let Some(entry) = cursor {
            path.push(entry);
            if entry.height <= first {
                break;
            }
            cursor = self.parent_of(&entry);
        }
        path.iter().rev().map_while(|entry| self.body_of(entry)).collect()
    }

    /// The best chain from `first` upward, as far as this node holds it. Reads every block, so a
    /// long walk should go through [`Chain::blocks_from`] in pieces.
    pub fn best_chain_from(&self, first: u64) -> Vec<Block> {
        let mut blocks = Vec::new();
        let mut next = first;
        loop {
            let part = self.blocks_from(next, 256);
            let Some(last) = part.len().checked_sub(1) else {
                return blocks;
            };
            next += last as u64 + 1;
            blocks.extend(part);
        }
    }

    /// The oldest block of the best chain whose body is still here.
    pub fn oldest_held(&self) -> u64 {
        let tail = self.log.tail();
        let tip = self.tip_entry();
        let next = self
            .marks
            .iter()
            .find(|mark| mark.at != NOWHERE && mark.at >= tail)
            .and_then(|mark| self.log.meta(mark.at).map(|meta| (mark, meta)));
        let start = next.map(|(mark, meta)| Entry {
            header: meta.header,
            height: meta.height,
            total_work: meta.total_work,
            at: mark.at,
            parent_at: meta.parent_at,
            validated: true,
        });
        let mut cursor = start.unwrap_or(tip);
        if cursor.at == NOWHERE {
            return if cursor.height == 0 { 0 } else { cursor.height + 1 };
        }
        loop {
            let Some(parent) = self.parent_of(&cursor) else {
                return cursor.height;
            };
            if parent.height == 0 {
                return 0;
            }
            if parent.at == NOWHERE || parent.at < tail {
                return cursor.height;
            }
            cursor = parent;
        }
    }

    /// How much of the chain this node holds, for showing.
    pub fn held(&self) -> Held {
        let oldest = self.oldest_held();
        let blocks = (self.height() + 1).saturating_sub(oldest).max(1);
        let bytes = self.log.held();
        let capacity = Some(self.log.capacity()).filter(|size| *size != u64::MAX);
        Held {
            oldest,
            bytes,
            capacity,
            recent: self.recent,
            fits: capacity.map(|size| {
                let each = (bytes / blocks).max(1);
                (size.saturating_sub(SECTOR) / each).max(FEWEST_RECENT)
            }),
        }
    }

    /// Changes how many recent blocks are kept. `None` keeps all that fit.
    pub fn set_recent(&mut self, recent: Option<u64>) {
        self.recent = recent.map(|blocks| blocks.max(FEWEST_RECENT));
        self.prune();
    }

    /// Lets go of bodies below the kept range, and never of what a boot still has to read.
    pub(crate) fn prune(&mut self) {
        let Some(recent) = self.recent else {
            return;
        };
        let Some(cutoff) = self.height().checked_sub(recent) else {
            return;
        };
        let next = self.marks.partition_point(|mark| mark.height <= cutoff);
        let Some(mark) = next.checked_sub(1).and_then(|last| self.marks.get(last)) else {
            return;
        };
        if mark.at != NOWHERE {
            self.log.forget(mark.at.min(self.saved_head));
        }
    }

    pub fn unspent_of(&self, address: &Address) -> Vec<(OutPoint, Unspent)> {
        self.unspent
            .iter()
            .filter(|(_, entry)| entry.output.lock.as_slice() == address.as_slice())
            .map(|(point, entry)| (*point, entry.clone()))
            .collect()
    }

    /// A hash of the whole unspent set. Two nodes with the same one agree on every coin.
    pub fn unspent_hash(&self) -> Hash {
        olc_crypto::hash(&postcard::to_allocvec(&self.unspent).unwrap_or_default())
    }

    /// How many outputs the whole coin holds unspent.
    pub fn unspent_count(&self) -> usize {
        self.unspent.len()
    }

    /// Spendable balance: coinbase outputs count once they have matured.
    /// Walks every unspent output, so callers on a hot path should keep the result.
    pub fn balance(&self, address: &Address) -> u64 {
        let next = self.height() + 1;
        self.unspent
            .values()
            .filter(|entry| entry.output.lock.as_slice() == address.as_slice())
            .filter(|entry| self.is_mature(entry, next))
            .map(|entry| entry.output.value)
            .sum()
    }

    pub fn is_mature(&self, entry: &Unspent, spend_height: u64) -> bool {
        !entry.coinbase || spend_height >= entry.height + self.config.rules.coinbase_maturity
    }

    /// The difficulty a block on top of `parent` must carry.
    pub fn next_difficulty(&self, parent: &Hash) -> u64 {
        match self.entries.get(parent) {
            Some(entry) => self.difficulty_after(entry),
            None => self.config.initial_difficulty,
        }
    }

    pub(crate) fn difficulty_after(&self, entry: &Entry) -> u64 {
        let rules = &self.config.rules;
        let interval = rules.retarget_interval;
        let next_height = entry.height + 1;
        if interval == 0 || next_height % interval != 0 || next_height < interval {
            return entry.header.difficulty;
        }
        let mut first = *entry;
        for _ in 1..interval {
            match self.parent_of(&first) {
                Some(previous) => first = previous,
                None => break,
            }
        }
        let input = RetargetInput {
            parent_difficulty: entry.header.difficulty,
            actual_ms: entry.header.time_ms.saturating_sub(first.header.time_ms),
            expected_ms: rules.block_time_ms * (interval - 1),
        };
        self.plugins.consensus.next_difficulty(&input)
    }

    /// Checks a transaction against the unspent set, as if mined at `height`. Returns its fee.
    pub fn check_tx(&self, tx: &Tx, height: u64) -> Result<u64, ChainError> {
        let view = Overlay::new(&self.unspent);
        rules::check_tx(tx, height, &view, &self.config.rules, &self.plugins)
    }

    /// False once `point` has been spent or was never made. Costs no signature checks.
    pub fn is_unspent(&self, point: &OutPoint) -> bool {
        self.unspent.contains_key(point)
    }

    /// The two sides of a move from `old_tip` to the best chain, each oldest block first: the
    /// blocks that left the best chain and the ones that joined it.
    pub fn branches_from(&self, old_tip: &Hash) -> (Vec<Block>, Vec<Block>) {
        let (mut left, mut joined) = (Vec::new(), Vec::new());
        let (mut old, mut new) = (self.entries.get(old_tip).copied(), Some(self.tip_entry()));
        while let (Some(from), Some(to)) = (old, new) {
            if from.header == to.header {
                break;
            }
            if from.height >= to.height {
                left.extend(self.body_of(&from));
                old = self.parent_of(&from);
            } else {
                joined.extend(self.body_of(&to));
                new = self.parent_of(&to);
            }
        }
        left.reverse();
        joined.reverse();
        (left, joined)
    }

    /// Checks `txs` in order for the next block, each seeing the outputs of those kept before it.
    /// Returns the ones that stay valid, then the rest with the reason each failed.
    pub fn recheck(&self, txs: Vec<Tx>) -> (Vec<Tx>, Vec<(Tx, ChainError)>) {
        let height = self.height() + 1;
        let rules = &self.config.rules;
        let mut view = Overlay::new(&self.unspent);
        let (mut kept, mut refused) = (Vec::new(), Vec::new());
        for tx in txs {
            match rules::check_tx(&tx, height, &view, rules, &self.plugins) {
                Ok(_) => {
                    view.apply(&tx, height, None);
                    kept.push(tx);
                }
                Err(reason) => refused.push((tx, reason)),
            }
        }
        (kept, refused)
    }

    /// Takes the orphans waiting for `parent`, so the caller can add them again.
    pub fn take_orphans_of(&mut self, parent: &Hash) -> Vec<Block> {
        self.orphans.take_children(parent)
    }

    /// The height of a recent transaction on the best chain.
    pub fn find_tx(&self, id: &Hash) -> Option<u64> {
        self.found.height_of(id)
    }

    /// Selects transactions and builds an unsealed block on the tip.
    pub fn block_template(&self, miner: &Address, pool: &[Tx], time_ms: u64) -> Block {
        self.block_template_with(miner, pool, time_ms, &[])
    }

    /// As [`Chain::block_template`], with `note` in a data output of the coinbase when it is not
    /// empty.
    pub fn block_template_with(
        &self,
        miner: &Address,
        pool: &[Tx],
        time_ms: u64,
        note: &[u8],
    ) -> Block {
        let height = self.height() + 1;
        let rules = &self.config.rules;
        let mut view = Overlay::new(&self.unspent);
        let mut chosen = Vec::new();
        let mut fees = 0u64;
        let mut bytes = 256 + note.len() as u64;
        for tx in pool {
            let size = tx.encode().len() as u64;
            if bytes + size > rules.max_block_bytes {
                continue;
            }
            if let Ok(fee) = rules::check_tx(tx, height, &view, rules, &self.plugins) {
                view.apply(tx, height, None);
                fees = fees.saturating_add(fee);
                bytes += size;
                chosen.push(tx.clone());
            }
        }
        let mut outputs = vec![TxOut {
            value: rules.reward_at(height).saturating_add(fees),
            language: 0,
            lock: miner.to_vec(),
        }];
        if !note.is_empty() {
            outputs.push(TxOut::data(note.to_vec()));
        }
        let coinbase = Tx {
            version: 1,
            inputs: Vec::new(),
            outputs,
            coinbase_height: Some(height),
        };
        let mut txs = vec![coinbase];
        txs.append(&mut chosen);
        let header = BlockHeader {
            version: 1,
            previous: self.tip,
            merkle_root: merkle_root(&txs),
            time_ms: time_ms.max(self.tip_header().time_ms + 1),
            difficulty: self.next_difficulty(&self.tip),
            nonce: 0,
        };
        Block { header, txs }
    }
}
