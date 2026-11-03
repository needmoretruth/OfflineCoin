//! The rules a transaction and a block body must meet. These are the coin's consensus: they decide
//! nothing about how a node stores anything.
use crate::chain::ChainError;
use crate::coin::{CoinConfig, Rules};
use crate::ledger::{Overlay, Unspent, UnspentSet};
use crate::plugin::{Plugins, SpendContext, Spending};
use crate::types::{Block, Tx, UNLOCK_FREE_VERSION};
use alloc::collections::BTreeSet;
use alloc::vec::Vec;
use olc_crypto::Hash;

/// What the coin asks of a block at one height.
pub(crate) struct AtHeight<'a> {
    pub(crate) height: u64,
    pub(crate) rules: &'a Rules,
    /// What a signature there names. See [`CoinConfig::signing_id_at`].
    pub(crate) coin: Option<Hash>,
    /// Version 2 transactions are valid there. See [`crate::coin::OLC_UPGRADE_FROM`].
    pub(crate) upgraded: bool,
    /// Version 1 transactions are valid there. See [`CoinConfig::takes_version_one_at`].
    pub(crate) version_one: bool,
}

impl<'a> AtHeight<'a> {
    pub(crate) fn of(config: &'a CoinConfig, height: u64) -> Self {
        Self {
            height,
            rules: config.rules_at(height),
            coin: config.signing_id_at(height),
            upgraded: config.upgraded_at(height),
            version_one: config.takes_version_one_at(height),
        }
    }
}

pub(crate) fn check_tx(
    tx: &Tx,
    at: &AtHeight,
    unspent: &Overlay,
    plugins: &Plugins,
) -> Result<u64, ChainError> {
    let (height, rules) = (at.height, at.rules);
    // Older builds name such a transaction by every byte, so it waits for the upgrade.
    let new_version = tx.version >= UNLOCK_FREE_VERSION;
    if new_version && !at.upgraded {
        return Err(ChainError::NotYetAllowed);
    }
    if !new_version && !at.version_one {
        return Err(ChainError::NeedsNewVersion);
    }
    // A version 1 transaction signs as it always did, naming no coin.
    let coin = at.coin.filter(|_| new_version);
    if !tx.final_at(height) {
        return Err(ChainError::NotFinal);
    }
    let mut seen = BTreeSet::new();
    let mut total_in = 0u64;
    for (index, input) in tx.inputs.iter().enumerate() {
        if !seen.insert(input.previous) {
            return Err(ChainError::DuplicateInput);
        }
        let spent = unspent
            .get(&input.previous)
            .ok_or(ChainError::MissingInput)?;
        if spent.coinbase && height < rules.matures_at(spent.height) {
            return Err(ChainError::ImmatureCoinbase);
        }
        let language = plugins
            .language(spent.output.language)
            .ok_or(ChainError::UnknownLanguage)?;
        // BIP 68: the coin must have stood as many blocks as the input's sequence asks.
        let held = tx.relative_lock(index).unwrap_or(0);
        if height < spent.height.saturating_add(held) {
            return Err(ChainError::NotFinal);
        }
        let lock = &spent.output.lock;
        let spending = coin.map(|coin| Spending { tx, input: index, lock, coin });
        let context = SpendContext {
            signature_hash: tx.signature_hash(index, lock, coin.as_ref()),
            height,
            spending,
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
    at: &AtHeight,
    unspent: &UnspentSet,
    plugins: &Plugins,
) -> Result<Vec<Unspent>, ChainError> {
    let (height, rules) = (at.height, at.rules);
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
        fees = fees.saturating_add(check_tx(tx, at, &working, plugins)?);
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
