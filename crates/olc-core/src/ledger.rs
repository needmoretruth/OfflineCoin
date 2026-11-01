//! The unspent set and what a block changes in it, both ways. Data outputs never enter it: nothing
//! can spend them, so a node has no reason to remember them.
use crate::types::{Block, OutPoint, Tx, TxOut};
use alloc::collections::{BTreeMap, BTreeSet};
use alloc::vec::Vec;
use olc_crypto::Hash;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Unspent {
    pub output: TxOut,
    pub height: u64,
    pub coinbase: bool,
}

pub(crate) type UnspentSet = BTreeMap<OutPoint, Unspent>;

/// Recent transactions and their heights. Answers "did my payment arrive" and stays small, so old
/// ones drop out. Coinbases stay out: one comes with every block.
#[derive(Clone, Default, Serialize, Deserialize)]
pub(crate) struct Found(BTreeMap<Hash, u64>);

const MOST_FOUND: usize = 4096;

impl Found {
    pub fn add(&mut self, block: &Block, height: u64) {
        for tx in block.txs.iter().skip(1) {
            self.0.insert(tx.id(), height);
        }
        if self.0.len() > MOST_FOUND {
            let mut heights: Vec<u64> = self.0.values().copied().collect();
            let middle = heights.len() / 2;
            let (_, oldest_kept, _) = heights.select_nth_unstable(middle);
            let cutoff = *oldest_kept;
            self.0.retain(|_, height| *height >= cutoff);
        }
    }

    pub fn remove(&mut self, block: &Block) {
        for tx in block.txs.iter().skip(1) {
            self.0.remove(&tx.id());
        }
    }

    pub fn height_of(&self, id: &Hash) -> Option<u64> {
        self.0.get(id).copied()
    }
}

/// The unspent set as seen part way through a block, without copying it.
pub(crate) struct Overlay<'a> {
    base: &'a UnspentSet,
    spent: BTreeSet<OutPoint>,
    made: BTreeMap<OutPoint, Unspent>,
}

impl<'a> Overlay<'a> {
    pub fn new(base: &'a UnspentSet) -> Self {
        Self {
            base,
            spent: BTreeSet::new(),
            made: BTreeMap::new(),
        }
    }

    pub fn get(&self, point: &OutPoint) -> Option<&Unspent> {
        if self.spent.contains(point) {
            return None;
        }
        self.made.get(point).or_else(|| self.base.get(point))
    }

    /// Spends and creates what `tx` does, and adds what it spent to `undo`.
    pub fn apply(&mut self, tx: &Tx, height: u64, undo: Option<&mut Vec<Unspent>>) {
        let mut undo = undo;
        for input in &tx.inputs {
            if let (Some(list), Some(entry)) = (undo.as_deref_mut(), self.get(&input.previous)) {
                list.push(entry.clone());
            }
            if self.made.remove(&input.previous).is_none() {
                self.spent.insert(input.previous);
            }
        }
        for (point, entry) in outputs_of(tx, height) {
            self.made.insert(point, entry);
        }
    }
}

fn outputs_of(tx: &Tx, height: u64) -> impl Iterator<Item = (OutPoint, Unspent)> + '_ {
    let txid = tx.id();
    let coinbase = tx.is_coinbase();
    let outputs = tx.outputs.iter().enumerate();
    outputs.filter(|(_, output)| !output.is_data()).map(move |(index, output)| {
        let point = OutPoint {
            txid,
            index: index as u32,
        };
        let entry = Unspent {
            output: output.clone(),
            height,
            coinbase,
        };
        (point, entry)
    })
}

/// Moves the unspent set forward over `block`.
pub(crate) fn apply_block(block: &Block, height: u64, unspent: &mut UnspentSet) {
    for tx in &block.txs {
        for input in &tx.inputs {
            unspent.remove(&input.previous);
        }
        unspent.extend(outputs_of(tx, height));
    }
}

/// What a block spends, in the order it spends it.
pub(crate) fn spent_by(block: &Block, height: u64, unspent: &UnspentSet) -> Vec<Unspent> {
    let mut overlay = Overlay::new(unspent);
    let mut spent = Vec::new();
    for tx in &block.txs {
        overlay.apply(tx, height, Some(&mut spent));
    }
    spent
}

/// Moves the unspent set back over `block`. `spent` comes from [`spent_by`].
pub(crate) fn undo_block(block: &Block, spent: &[Unspent], unspent: &mut UnspentSet) {
    let mut restored = spent.iter().rev();
    for tx in block.txs.iter().rev() {
        let txid = tx.id();
        for index in 0..tx.outputs.len() as u32 {
            unspent.remove(&OutPoint { txid, index });
        }
        for input in tx.inputs.iter().rev() {
            if let Some(entry) = restored.next() {
                unspent.insert(input.previous, entry.clone());
            }
        }
    }
}
