//! Blocks to mine, and the pool's view of payments that wait on other waiting payments. A block
//! takes payments by the fee rate of each one together with the waiting payments it needs, as
//! Bitcoin Core's ancestor-feerate selection does, so a child paying well brings in a parent
//! that pays little (child pays for parent).
use super::{Chain, BLOCK_OVERHEAD};
use crate::ledger::Overlay;
use crate::rules::{self, AtHeight};
use crate::types::{merkle_root, Block, BlockHeader, OutPoint, Tx, TxOut};
use alloc::collections::{BTreeMap, BTreeSet};
use alloc::vec;
use alloc::vec::Vec;
use olc_crypto::{Address, Hash};

/// Bytes a pool may fill in a coinbase, as Stratum's extranonce 1 and 2 of four bytes each.
pub const EXTRANONCE_BYTES: usize = 8;

/// True for a coinbase's extranonce: its last output, a data output of [`EXTRANONCE_BYTES`]. It
/// holds a miner's counter, not words anyone wrote.
pub fn is_extranonce(tx: &Tx, index: usize) -> bool {
    let output = tx.outputs.get(index).filter(|_| tx.coinbase_height.is_some() && index + 1 == tx.outputs.len());
    output.is_some_and(|output| output.is_data() && output.lock.len() == EXTRANONCE_BYTES)
}

/// A coinbase's bytes on either side of its extranonce, as Stratum's `coinb1` and `coinb2`.
/// `None` when its last output is not an extranonce.
pub fn coinbase_split(coinbase: &Tx) -> Option<(Vec<u8>, Vec<u8>)> {
    let last = coinbase.outputs.len().checked_sub(1).filter(|at| is_extranonce(coinbase, *at))?;
    let last = coinbase.outputs.get(last)?;
    let bytes = coinbase.encode();
    // The extranonce is where the bytes change when only it does.
    let mut other = coinbase.clone();
    let flipped = last.lock.iter().map(|byte| !byte).collect();
    other.outputs.last_mut()?.lock = flipped;
    let at = bytes.iter().zip(other.encode()).position(|(ours, theirs)| *ours != theirs)?;
    (bytes.get(at..at + EXTRANONCE_BYTES)? == last.lock.as_slice())
        .then(|| (bytes[..at].to_vec(), bytes[at + EXTRANONCE_BYTES..].to_vec()))
}

/// What one waiting payment adds to a block: its fee, its size, and the waiting payments it
/// spends from, by their place in the pool.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Waiting {
    pub fee: u64,
    pub size: u64,
    pub parents: Vec<usize>,
}

/// True when `fee` over `size` pays more per byte than `other_fee` over `other_size`.
pub fn pays_more(fee: u64, size: u64, other_fee: u64, other_size: u64) -> bool {
    u128::from(fee) * u128::from(other_size.max(1)) > u128::from(other_fee) * u128::from(size.max(1))
}

/// Every waiting payment `at` needs, itself included, oldest first.
pub fn ancestors(waiting: &[Waiting], at: usize) -> BTreeSet<usize> {
    let mut found = BTreeSet::new();
    let mut next = vec![at];
    while let Some(place) = next.pop() {
        if found.insert(place) {
            next.extend(waiting.get(place).map_or(&[][..], |entry| &entry.parents));
        }
    }
    found
}

/// The order a block takes waiting payments in: at each step the payment whose fee rate,
/// counted with the waiting payments it needs, is highest, those first. A package that no longer
/// fits in `room` bytes is passed over.
pub fn by_package(waiting: &[Waiting], room: u64) -> Vec<usize> {
    let needs: Vec<BTreeSet<usize>> = (0..waiting.len()).map(|at| ancestors(waiting, at)).collect();
    let mut taken = vec![false; waiting.len()];
    let mut passed = vec![false; waiting.len()];
    let mut order = Vec::new();
    let mut left = room;
    loop {
        let mut best: Option<(usize, u64, u64)> = None;
        for at in (0..waiting.len()).filter(|at| !taken[*at] && !passed[*at]) {
            let open = needs[at].iter().filter(|need| !taken[**need]);
            let (fee, size) = open.fold((0u64, 0u64), |(fee, size), need| {
                (fee.saturating_add(waiting[*need].fee), size.saturating_add(waiting[*need].size))
            });
            if best.is_none_or(|(_, best_fee, best_size)| pays_more(fee, size, best_fee, best_size)) {
                best = Some((at, fee, size));
            }
        }
        let Some((at, _, size)) = best else {
            return order;
        };
        if size > left {
            passed[at] = true;
            continue;
        }
        left -= size;
        for need in &needs[at] {
            if !taken[*need] {
                taken[*need] = true;
                order.push(*need);
            }
        }
    }
}

impl Chain {
    /// The fee, size and waiting parents of each payment in `pool`, read without running a
    /// script: what an input spends comes from the unspent set or from a payment before it.
    pub fn waiting(&self, pool: &[Tx]) -> Vec<Waiting> {
        let places: BTreeMap<Hash, usize> = pool.iter().enumerate().map(|(at, tx)| (tx.id(), at)).collect();
        let value_of = |point: &OutPoint| match places.get(&point.txid) {
            Some(at) => pool[*at].outputs.get(point.index as usize).map(|output| output.value),
            None => self.unspent.get(point).map(|entry| entry.output.value),
        };
        let entry = |tx: &Tx| {
            let spent = tx.inputs.iter().map(|input| value_of(&input.previous).unwrap_or(0));
            let made = tx.outputs.iter().map(|output| output.value);
            let fee = spent.fold(0, u64::saturating_add).saturating_sub(made.fold(0, u64::saturating_add));
            let parents: BTreeSet<usize> = tx.inputs.iter().filter_map(|input| places.get(&input.previous.txid).copied()).collect();
            Waiting { fee, size: tx.encode().len() as u64, parents: parents.into_iter().collect() }
        };
        pool.iter().map(entry).collect()
    }

    /// As [`Chain::check_tx`], with the payments of `before` already made: their outputs can be
    /// spent and what they spend cannot.
    pub fn check_tx_after<'a>(&self, before: impl IntoIterator<Item = &'a Tx>, tx: &Tx, height: u64) -> Result<u64, crate::ChainError> {
        let mut view = Overlay::new(&self.unspent);
        for earlier in before {
            view.apply(earlier, height, None);
        }
        rules::check_tx(tx, &AtHeight::of(&self.config, height), &view, &self.plugins)
    }

    /// As [`Chain::signature_checks`], where an input may spend an output of `before`.
    pub fn signature_checks_after<'a>(&self, before: impl IntoIterator<Item = &'a Tx>, tx: &Tx) -> usize {
        let made: BTreeMap<Hash, &Tx> = before.into_iter().map(|earlier| (earlier.id(), earlier)).collect();
        let of_input = |input: &crate::types::TxIn| {
            let previous = &input.previous;
            let made_here = made.get(&previous.txid).and_then(|earlier| earlier.outputs.get(previous.index as usize));
            let output = made_here.or_else(|| self.unspent.get(previous).map(|entry| &entry.output))?;
            let language = self.plugins.language(output.language)?;
            Some(language.signature_checks(&output.lock, &input.unlock))
        };
        tx.inputs.iter().filter_map(of_input).fold(0, usize::saturating_add)
    }

    /// Bytes a block has for payments.
    pub fn room_for_payments(&self, note: usize) -> u64 {
        self.rules().max_block_bytes.saturating_sub(BLOCK_OVERHEAD + note as u64)
    }

    /// Selects transactions and builds an unsealed block on the tip.
    pub fn block_template(&self, miner: &Address, pool: &[Tx], time_ms: u64) -> Block {
        self.block_template_with(miner, pool, time_ms, &[])
    }

    /// As [`Chain::block_template`], with `note` in a data output of the coinbase when it is not
    /// empty. On a coin with Bitcoin's header the coinbase's last output holds
    /// [`EXTRANONCE_BYTES`] zero bytes as data, which a pool fills as Stratum's extranonce: its
    /// `coinb1` is the coinbase up to them and its `coinb2` what follows, and the coinbase's id
    /// is the double SHA-256 of the three together, as in Bitcoin.
    pub fn block_template_with(&self, miner: &Address, pool: &[Tx], time_ms: u64, note: &[u8]) -> Block {
        let height = self.height() + 1;
        let at = AtHeight::of(&self.config, height);
        let rules = at.rules;
        let mut view = Overlay::new(&self.unspent);
        let mut chosen = Vec::new();
        let mut fees = 0u64;
        let mut bytes = BLOCK_OVERHEAD + note.len() as u64;
        let order = by_package(&self.waiting(pool), self.room_for_payments(note.len()));
        for tx in order.into_iter().map(|at| &pool[at]) {
            let size = tx.encode().len() as u64;
            if bytes + size > rules.max_block_bytes {
                continue;
            }
            // One that fails here takes its children with it: their inputs are then missing.
            if let Ok(fee) = rules::check_tx(tx, &at, &view, &self.plugins) {
                view.apply(tx, height, None);
                fees = fees.saturating_add(fee);
                bytes += size;
                chosen.push(tx.clone());
            }
        }
        let mut outputs = vec![TxOut { value: rules.reward_at(height).saturating_add(fees), language: 0, lock: miner.to_vec() }];
        if !note.is_empty() {
            outputs.push(TxOut::data(note.to_vec()));
        }
        if self.config.bitcoin_header {
            outputs.push(TxOut::data(vec![0; EXTRANONCE_BYTES]));
        }
        let coinbase = Tx { version: 1, inputs: Vec::new(), outputs, coinbase_height: Some(height), lock_time: 0 };
        let mut txs = vec![coinbase];
        txs.append(&mut chosen);
        let whole = match self.config.bitcoin_header {
            true => time_ms / 1_000 * 1_000,
            false => time_ms,
        };
        let time_ms = whole.max(self.earliest_time(&self.tip));
        let header = BlockHeader {
            version: self.header_version_at(height),
            previous: self.tip,
            merkle_root: merkle_root(&txs),
            time_ms,
            difficulty: self.next_difficulty_at(&self.tip, time_ms),
            nonce: 0,
        };
        Block { header, txs }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(fee: u64, size: u64, parents: &[usize]) -> Waiting {
        Waiting { fee, size, parents: parents.to_vec() }
    }

    #[test]
    fn a_child_paying_well_brings_its_parent_in_ahead_of_others() {
        // A parent paying nothing, three paying 2 a byte, and a child paying 10 a byte.
        let pool = [entry(0, 100, &[]), entry(200, 100, &[]), entry(200, 100, &[]), entry(200, 100, &[]), entry(1_000, 100, &[0])];
        assert_eq!(by_package(&pool, 200), vec![0, 4]);
        // Alone, the parent waits behind every other payment.
        let alone = &pool[..4];
        assert_eq!(by_package(alone, 200), vec![1, 2]);
    }

    #[test]
    fn a_package_too_big_for_the_room_left_is_passed_over() {
        let pool = [entry(1_000, 300, &[]), entry(10, 100, &[])];
        assert_eq!(by_package(&pool, 200), vec![1]);
        assert!(pays_more(3, 1, 2, 1) && !pays_more(2, 1, 2, 1));
    }
}
