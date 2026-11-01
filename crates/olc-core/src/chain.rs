//! Stores blocks, follows the chain with the most work and tracks unspent outputs.
//!
//! The rule for simplicity: after any reorganisation the unspent set is rebuilt from genesis.
//! That is slow on a chain with millions of blocks and instant on the chains this coin is for.
use crate::coin::{CoinConfig, Rules};
use crate::orphans::Orphans;
use crate::plugin::{Plugins, RetargetInput, SpendContext};
use crate::types::{merkle_root, Block, BlockHeader, OutPoint, Tx, TxOut};
use alloc::collections::{BTreeMap, BTreeSet};
use alloc::vec;
use alloc::vec::Vec;
use olc_crypto::{Address, Hash};

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

struct Entry {
    block: Block,
    height: u64,
    total_work: u128,
    /// Set once the transactions have been checked against this block's history.
    validated: bool,
}

#[derive(Clone, Debug)]
pub struct Unspent {
    pub output: TxOut,
    pub height: u64,
    pub coinbase: bool,
}

type UnspentSet = BTreeMap<OutPoint, Unspent>;

pub struct Chain {
    config: CoinConfig,
    plugins: Plugins,
    genesis: Hash,
    entries: BTreeMap<Hash, Entry>,
    orphans: Orphans,
    tip: Hash,
    unspent: UnspentSet,
}

impl Chain {
    pub fn new(config: CoinConfig, plugins: Plugins) -> Self {
        let block = config.genesis();
        let id = block.id();
        let mut unspent = UnspentSet::new();
        apply_block(&block, 0, &mut unspent);
        let entry = Entry {
            block,
            height: 0,
            total_work: 0,
            validated: true,
        };
        let mut entries = BTreeMap::new();
        entries.insert(id, entry);
        Self {
            config,
            plugins,
            genesis: id,
            entries,
            orphans: Orphans::default(),
            tip: id,
            unspent,
        }
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

    pub fn tip_header(&self) -> &BlockHeader {
        &self.entries[&self.tip].block.header
    }

    pub fn height(&self) -> u64 {
        self.entries[&self.tip].height
    }

    pub fn total_work(&self) -> u128 {
        self.entries[&self.tip].total_work
    }

    pub fn contains(&self, id: &Hash) -> bool {
        self.entries.contains_key(id) || self.orphans.contains(id)
    }

    pub fn block(&self, id: &Hash) -> Option<&Block> {
        self.entries
            .get(id)
            .map(|entry| &entry.block)
            .or_else(|| self.orphans.get(id))
    }

    /// The best chain from `first` upward, in order.
    pub fn best_chain_from(&self, first: u64) -> Vec<&Block> {
        let mut blocks = Vec::new();
        let mut cursor = self.tip;
        while let Some(entry) = self.entries.get(&cursor) {
            if entry.height < first {
                break;
            }
            blocks.push(&entry.block);
            if entry.height == 0 {
                break;
            }
            cursor = entry.block.header.previous;
        }
        blocks.reverse();
        blocks
    }

    pub fn unspent_of(&self, address: &Address) -> Vec<(OutPoint, Unspent)> {
        self.unspent
            .iter()
            .filter(|(_, entry)| entry.output.lock.as_slice() == address.as_slice())
            .map(|(point, entry)| (*point, entry.clone()))
            .collect()
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
        let Some(entry) = self.entries.get(parent) else {
            return self.config.initial_difficulty;
        };
        let rules = &self.config.rules;
        let interval = rules.retarget_interval;
        let next_height = entry.height + 1;
        if interval == 0 || next_height % interval != 0 || next_height < interval {
            return entry.block.header.difficulty;
        }
        let mut first = entry;
        for _ in 1..interval {
            match self.entries.get(&first.block.header.previous) {
                Some(previous) => first = previous,
                None => break,
            }
        }
        let input = RetargetInput {
            parent_difficulty: entry.block.header.difficulty,
            actual_ms: entry
                .block
                .header
                .time_ms
                .saturating_sub(first.block.header.time_ms),
            expected_ms: rules.block_time_ms * (interval - 1),
        };
        self.plugins.consensus.next_difficulty(&input)
    }

    /// Checks a transaction against the unspent set, as if mined at `height`. Returns its fee.
    pub fn check_tx(&self, tx: &Tx, height: u64) -> Result<u64, ChainError> {
        check_tx(tx, height, &self.unspent, &self.config.rules, &self.plugins)
    }

    /// Adds a block and follows it if its branch now has the most work.
    pub fn add_block(&mut self, block: Block) -> Result<AddOutcome, ChainError> {
        let id = block.id();
        if self.entries.contains_key(&id) {
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
        let Some(parent) = self.entries.get(&previous) else {
            self.orphans.insert(id, block);
            return Ok(AddOutcome::Orphan { missing: previous });
        };
        if block.header.time_ms <= parent.block.header.time_ms {
            return Err(ChainError::BadTime);
        }
        if block.header.difficulty != self.next_difficulty(&previous) {
            return Err(ChainError::BadDifficulty);
        }
        let height = parent.height + 1;
        let total_work = parent.total_work + self.plugins.consensus.work(&block.header);
        let extends_tip = previous == self.tip;

        if extends_tip {
            check_block_body(
                &block,
                height,
                &self.unspent,
                &self.config.rules,
                &self.plugins,
            )?;
            apply_block(&block, height, &mut self.unspent);
            self.entries.insert(
                id,
                Entry {
                    block,
                    height,
                    total_work,
                    validated: true,
                },
            );
            self.tip = id;
            return Ok(AddOutcome::NewTip);
        }

        self.entries.insert(
            id,
            Entry {
                block,
                height,
                total_work,
                validated: false,
            },
        );
        if total_work <= self.total_work() {
            return Ok(AddOutcome::SideBranch);
        }
        let old_height = self.height();
        let fork_height = self.fork_height(&id);
        match self.replay(&id) {
            Ok(()) => Ok(AddOutcome::Reorganized {
                replaced: old_height - fork_height,
            }),
            Err(error) => {
                self.entries.remove(&id);
                Err(error)
            }
        }
    }

    /// False once `point` has been spent or was never made. Costs no signature checks.
    pub fn is_unspent(&self, point: &OutPoint) -> bool {
        self.unspent.contains_key(point)
    }

    /// The two sides of a move from `old_tip` to the best chain, each oldest block first: the
    /// blocks that left the best chain and the ones that joined it.
    pub fn branches_from(&self, old_tip: &Hash) -> (Vec<&Block>, Vec<&Block>) {
        let (mut left, mut joined) = (Vec::new(), Vec::new());
        let (mut old, mut new) = (self.entries.get(old_tip), self.entries.get(&self.tip));
        while let (Some(from), Some(to)) = (old, new) {
            if from.block.id() == to.block.id() {
                break;
            }
            if from.height >= to.height {
                left.push(&from.block);
                old = self.entries.get(&from.block.header.previous);
            }
            if to.height >= from.height {
                joined.push(&to.block);
                new = self.entries.get(&to.block.header.previous);
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
        let mut unspent = self.unspent.clone();
        let (mut kept, mut refused) = (Vec::new(), Vec::new());
        for tx in txs {
            match check_tx(&tx, height, &unspent, rules, &self.plugins) {
                Ok(_) => {
                    spend_and_create(&tx, height, &mut unspent);
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

    /// When `id` is an orphan, returns the block its branch is still waiting for.
    pub fn missing_ancestor(&self, id: &Hash) -> Option<Hash> {
        let mut cursor = *id;
        while let Some(block) = self.orphans.get(&cursor) {
            cursor = block.header.previous;
        }
        (!self.entries.contains_key(&cursor)).then_some(cursor)
    }

    /// The height of a transaction on the best chain. Hashes every transaction, so it is for
    /// someone asking, not for a loop.
    pub fn find_tx(&self, id: &Hash) -> Option<u64> {
        let mut cursor = self.tip;
        while let Some(entry) = self.entries.get(&cursor) {
            if entry.block.txs.iter().any(|tx| tx.id() == *id) {
                return Some(entry.height);
            }
            if entry.height == 0 {
                return None;
            }
            cursor = entry.block.header.previous;
        }
        None
    }

    /// Selects transactions and builds an unsealed block on the tip.
    pub fn block_template(&self, miner: &Address, pool: &[Tx], time_ms: u64) -> Block {
        let height = self.height() + 1;
        let rules = &self.config.rules;
        // Copying the unspent set is only worth it when there is something to check.
        let mut unspent = if pool.is_empty() {
            UnspentSet::new()
        } else {
            self.unspent.clone()
        };
        let mut chosen = Vec::new();
        let mut fees = 0u64;
        let mut bytes = 256u64;
        for tx in pool {
            let size = tx.encode().len() as u64;
            if bytes + size > rules.max_block_bytes {
                continue;
            }
            if let Ok(fee) = check_tx(tx, height, &unspent, rules, &self.plugins) {
                spend_and_create(tx, height, &mut unspent);
                fees = fees.saturating_add(fee);
                bytes += size;
                chosen.push(tx.clone());
            }
        }
        let coinbase = Tx {
            version: 1,
            inputs: Vec::new(),
            outputs: vec![TxOut {
                value: rules.reward_at(height).saturating_add(fees),
                language: 0,
                lock: miner.to_vec(),
            }],
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

    fn fork_height(&self, new_tip: &Hash) -> u64 {
        let mut on_best = BTreeSet::new();
        let mut cursor = self.tip;
        while let Some(entry) = self.entries.get(&cursor) {
            on_best.insert(cursor);
            if entry.height == 0 {
                break;
            }
            cursor = entry.block.header.previous;
        }
        let mut cursor = *new_tip;
        while let Some(entry) = self.entries.get(&cursor) {
            if on_best.contains(&cursor) {
                return entry.height;
            }
            cursor = entry.block.header.previous;
        }
        0
    }

    /// Replays the branch ending at `new_tip` from genesis and makes it the best chain.
    /// Leaves the current state untouched when a block on the branch is invalid.
    fn replay(&mut self, new_tip: &Hash) -> Result<(), ChainError> {
        let mut path = Vec::new();
        let mut cursor = *new_tip;
        while let Some(entry) = self.entries.get(&cursor) {
            path.push(cursor);
            if entry.height == 0 {
                break;
            }
            cursor = entry.block.header.previous;
        }
        path.reverse();
        let mut unspent = UnspentSet::new();
        for id in &path {
            let entry = &self.entries[id];
            if !entry.validated {
                check_block_body(
                    &entry.block,
                    entry.height,
                    &unspent,
                    &self.config.rules,
                    &self.plugins,
                )?;
            }
            apply_block(&entry.block, entry.height, &mut unspent);
        }
        for id in &path {
            if let Some(entry) = self.entries.get_mut(id) {
                entry.validated = true;
            }
        }
        self.unspent = unspent;
        self.tip = *new_tip;
        Ok(())
    }
}

fn check_tx(
    tx: &Tx,
    height: u64,
    unspent: &UnspentSet,
    rules: &Rules,
    plugins: &Plugins,
) -> Result<u64, ChainError> {
    let mut seen = BTreeSet::new();
    let mut total_in = 0u64;
    for (index, input) in tx.inputs.iter().enumerate() {
        if !seen.insert(input.previous) {
            return Err(ChainError::DuplicateInput);
        }
        let spent = unspent
            .get(&input.previous)
            .ok_or(ChainError::MissingInput)?;
        if spent.coinbase && height < spent.height + rules.coinbase_maturity {
            return Err(ChainError::ImmatureCoinbase);
        }
        let language = plugins
            .language(spent.output.language)
            .ok_or(ChainError::UnknownLanguage)?;
        let context = SpendContext {
            signature_hash: tx.signature_hash(index, &spent.output.lock),
            height,
        };
        if !language.check(&spent.output.lock, &input.unlock, &context) {
            return Err(ChainError::BadLock);
        }
        total_in = total_in.saturating_add(spent.output.value);
    }
    let total_out = tx
        .outputs
        .iter()
        .fold(0u64, |sum, out| sum.saturating_add(out.value));
    let fee = total_in
        .checked_sub(total_out)
        .ok_or(ChainError::Overspend)?;
    if fee < rules.min_fee {
        return Err(ChainError::FeeTooLow);
    }
    Ok(fee)
}

fn check_block_body(
    block: &Block,
    height: u64,
    unspent: &UnspentSet,
    rules: &Rules,
    plugins: &Plugins,
) -> Result<(), ChainError> {
    let (coinbase, rest) = block.txs.split_first().ok_or(ChainError::NoCoinbase)?;
    if !coinbase.is_coinbase() || coinbase.coinbase_height != Some(height) {
        return Err(ChainError::BadCoinbase);
    }
    let mut fees = 0u64;
    if !rest.is_empty() {
        let mut working = unspent.clone();
        for tx in rest {
            if tx.is_coinbase() {
                return Err(ChainError::BadCoinbase);
            }
            let fee = check_tx(tx, height, &working, rules, plugins)?;
            fees = fees.saturating_add(fee);
            spend_and_create(tx, height, &mut working);
        }
    }
    let claimed = coinbase
        .outputs
        .iter()
        .fold(0u64, |sum, out| sum.saturating_add(out.value));
    if claimed > rules.reward_at(height).saturating_add(fees) {
        return Err(ChainError::BadCoinbase);
    }
    Ok(())
}

fn spend_and_create(tx: &Tx, height: u64, unspent: &mut UnspentSet) {
    for input in &tx.inputs {
        unspent.remove(&input.previous);
    }
    let txid = tx.id();
    for (index, output) in tx.outputs.iter().enumerate() {
        let point = OutPoint {
            txid,
            index: index as u32,
        };
        let entry = Unspent {
            output: output.clone(),
            height,
            coinbase: tx.is_coinbase(),
        };
        unspent.insert(point, entry);
    }
}

fn apply_block(block: &Block, height: u64, unspent: &mut UnspentSet) {
    for tx in &block.txs {
        spend_and_create(tx, height, unspent);
    }
}
