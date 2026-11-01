//! The rules a transaction and a block body must meet. These are the coin's consensus: they decide
//! nothing about how a node stores anything.
use crate::chain::ChainError;
use crate::coin::Rules;
use crate::ledger::{Overlay, Unspent, UnspentSet};
use crate::plugin::{Plugins, SpendContext};
use crate::types::{Block, Tx};
use alloc::collections::BTreeSet;
use alloc::vec::Vec;

pub(crate) fn check_tx(
    tx: &Tx,
    height: u64,
    unspent: &Overlay,
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

/// Checks a block's transactions and returns what it spends, which is what undoing it takes.
pub(crate) fn check_block_body(
    block: &Block,
    height: u64,
    unspent: &UnspentSet,
    rules: &Rules,
    plugins: &Plugins,
) -> Result<Vec<Unspent>, ChainError> {
    let (coinbase, rest) = block.txs.split_first().ok_or(ChainError::NoCoinbase)?;
    if !coinbase.is_coinbase() || coinbase.coinbase_height != Some(height) {
        return Err(ChainError::BadCoinbase);
    }
    let mut fees = 0u64;
    let mut working = Overlay::new(unspent);
    let mut spent = Vec::new();
    for tx in rest {
        if tx.is_coinbase() {
            return Err(ChainError::BadCoinbase);
        }
        fees = fees.saturating_add(check_tx(tx, height, &working, rules, plugins)?);
        working.apply(tx, height, Some(&mut spent));
    }
    let claimed = coinbase
        .outputs
        .iter()
        .fold(0u64, |sum, out| sum.saturating_add(out.value));
    if claimed > rules.reward_at(height).saturating_add(fees) {
        return Err(ChainError::BadCoinbase);
    }
    Ok(spent)
}
