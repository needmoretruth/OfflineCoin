//! What a block changes once it joins the best chain.
use crate::chain::{Unspent, UnspentSet};
use crate::types::{Block, OutPoint, Tx};
use alloc::collections::BTreeMap;
use olc_crypto::Hash;

/// Where each transaction of the best chain sits, so looking one up hashes nothing. Rebuilt with
/// the unspent set after a reorganisation.
pub(crate) type TxHeights = BTreeMap<Hash, u64>;

pub(crate) fn spend_and_create(tx: &Tx, height: u64, unspent: &mut UnspentSet) -> Hash {
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
    txid
}

pub(crate) fn apply_block(
    block: &Block,
    height: u64,
    unspent: &mut UnspentSet,
    tx_heights: &mut TxHeights,
) {
    for tx in &block.txs {
        let txid = spend_and_create(tx, height, unspent);
        tx_heights.insert(txid, height);
    }
}
