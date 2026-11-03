//! The pool: payments that wait for a block. What it takes comes from anyone in reach, so it has a
//! limit in bytes as well as in payments, and the cheap checks come before the signature.
use crate::engine::{Engine, PeerId};
use crate::event::{Event, Outgoing};
use crate::message::Message;
use alloc::collections::BTreeSet;
use alloc::vec::Vec;
use olc_core::{ancestors, pays_more, ChainError, Hash, OutPoint, Tx};
use olc_lang::words::MOST_SIGNATURE_CHECKS;

const MAX_POOL: usize = 256;
/// What a replacement pays for each of its own bytes beyond all it replaces: Bitcoin Core's
/// incremental relay fee, one unit a byte.
pub const INCREMENTAL_FEE: u64 = 1;
/// Most waiting payments one replacement may push out, as Bitcoin's 100.
pub const MOST_REPLACED: usize = 100;
/// Most waiting payments in a row that each spend the one before, as Bitcoin's 25. A block takes
/// a payment with every waiting payment it needs, and this keeps that work small on a board.
pub const MOST_IN_A_CHAIN: usize = 25;
/// What waiting payments may take in all. A board's heap sets this, and a computer gets no more
/// because it would only pass on what the boards cannot keep.
pub const MOST_POOL_BYTES: usize = 64 * 1024;
/// A data output larger than this stays out of this node's pool. Like Bitcoin's OP_RETURN limit
/// this is a node's policy, and a block holding a larger one is still valid.
pub const MOST_DATA: usize = 256;

impl Engine {
    /// The payments waiting for a block.
    pub fn pool(&self) -> &[Tx] {
        &self.pool
    }

    /// True while a transaction waits in this node's pool.
    pub fn in_pool(&self, id: &Hash) -> bool {
        self.pool.iter().any(|tx| tx.id() == *id)
    }

    /// Empties this node's pool, as its owner may. Its coins can be spent anew.
    pub fn clear_pool(&mut self) {
        self.pool.clear();
        self.candidate = None;
    }

    /// Hands in a transaction from outside the network, such as a forged test transaction.
    pub fn submit_tx(&mut self, tx: Tx) -> bool {
        self.receive_tx(None, tx)
    }

    /// The places of the waiting payments `tx` would push out, or why the pool has no place for
    /// it, whatever the chain says about it. These checks come first because a signature costs
    /// a board far more than all of them.
    ///
    /// As Bitcoin Core 28 does by default, a payment may replace any waiting one that spends the
    /// same coin (full replace-by-fee), on BIP 125's terms: it pays a higher fee rate than each it
    /// conflicts with, a total fee at least that of everything it pushes out, and on top of that
    /// [`INCREMENTAL_FEE`] for each of its own bytes, so a stream of replacements always costs.
    fn pool_refuses(&self, tx: &Tx) -> Result<BTreeSet<usize>, ChainError> {
        let size = tx.encode().len();
        if !self.chain.fits_a_block(size) {
            return Err(ChainError::TooLarge);
        }
        let spends_same = |known: &Tx| known.inputs.iter().any(|input| tx.inputs.iter().any(|new| new.previous == input.previous));
        let conflicts: BTreeSet<usize> = (0..self.pool.len()).filter(|at| spends_same(&self.pool[*at])).collect();
        let waiting = self.chain.waiting(&self.pool);
        let mut replaced = conflicts.clone();
        for (at, entry) in waiting.iter().enumerate() {
            if entry.parents.iter().any(|parent| replaced.contains(parent)) {
                replaced.insert(at);
            }
        }
        if replaced.len() > MOST_REPLACED {
            return Err(ChainError::DuplicateInput);
        }
        let kept = || (0..self.pool.len()).filter(|at| !replaced.contains(at));
        if !conflicts.is_empty() {
            // BIP 125's second rule: no waiting payment it spends from that the ones it replaces
            // did not already spend from.
            let spent_before = |point: &OutPoint| conflicts.iter().any(|at| self.pool[*at].inputs.iter().any(|input| input.previous == *point));
            let waits_on = |point: &OutPoint| kept().any(|at| self.pool[at].id() == point.txid);
            if tx.inputs.iter().any(|input| waits_on(&input.previous) && !spent_before(&input.previous)) {
                return Err(ChainError::DuplicateInput);
            }
            // Read in place: a copy of a full pool would not fit beside it on a board.
            let made_by = |point: &OutPoint| kept().find(|at| self.pool[*at].id() == point.txid);
            let value_of = |point: &OutPoint| match made_by(point) {
                Some(at) => self.pool[at].outputs.get(point.index as usize).map(|output| output.value),
                None => self.chain.coin_at(point).map(|coin| coin.output.value),
            };
            let spent = tx.inputs.iter().map(|input| value_of(&input.previous).ok_or(ChainError::MissingInput));
            let spent = spent.sum::<Result<u64, ChainError>>()?;
            let fee = spent.saturating_sub(tx.outputs.iter().map(|output| output.value).sum());
            let new = olc_core::Waiting { fee, size: size as u64, parents: Vec::new() };
            let pushed_out = replaced.iter().map(|at| waiting[*at].fee).fold(0, u64::saturating_add);
            let extra = new.fee.checked_sub(pushed_out).ok_or(ChainError::FeeTooLow)?;
            let beats = |at: &usize| pays_more(new.fee, new.size, waiting[*at].fee, waiting[*at].size);
            if extra < INCREMENTAL_FEE.saturating_mul(new.size) || !conflicts.iter().all(beats) {
                return Err(ChainError::FeeTooLow);
            }
        }
        let parents: Vec<usize> = kept().filter(|at| tx.inputs.iter().any(|input| input.previous.txid == self.pool[*at].id())).collect();
        let mut in_a_row = BTreeSet::new();
        for parent in parents {
            in_a_row.extend(ancestors(&waiting, parent));
        }
        if in_a_row.len() + 1 > MOST_IN_A_CHAIN {
            return Err(ChainError::TooLarge);
        }
        let held: usize = kept().map(|at| waiting[at].size as usize).sum();
        if held.saturating_add(size) > MOST_POOL_BYTES || kept().count() >= MAX_POOL {
            return Err(ChainError::TooLarge);
        }
        Ok(replaced)
    }

    pub(crate) fn receive_tx(&mut self, from: Option<PeerId>, tx: Tx) -> bool {
        if tx.is_coinbase() || self.pool.iter().any(|known| known == &tx) {
            return false;
        }
        if tx.outputs.iter().any(|out| out.is_data() && out.lock.len() > MOST_DATA) {
            self.push_event(Event::DataRefused);
            return false;
        }
        let replaced = match self.pool_refuses(&tx) {
            Ok(replaced) => replaced,
            Err(reason) => {
                self.push_event(Event::TxRejected { reason });
                return false;
            }
        };
        let kept = || self.pool.iter().enumerate().filter(|(at, _)| !replaced.contains(at)).map(|(_, known)| known);
        let checks = self.chain.signature_checks_after(kept(), &tx);
        if checks > MOST_SIGNATURE_CHECKS {
            self.push_event(Event::ChecksRefused { checks });
            return false;
        }
        match self.chain.check_tx_after(kept(), &tx, self.chain.height() + 1) {
            Ok(_) => {
                self.push_event(Event::TxAccepted);
                let old = core::mem::take(&mut self.pool);
                for (at, known) in old.into_iter().enumerate() {
                    if replaced.contains(&at) {
                        let reason = Some(ChainError::DuplicateInput);
                        self.push_event(Event::TxDropped { id: known.id(), reason });
                    } else {
                        self.pool.push(known);
                    }
                }
                self.pool.push(tx.clone());
                self.candidate = None;
                let message = Message::Tx(tx);
                self.outbox.push_back(Outgoing::All {
                    except: from,
                    message,
                });
                true
            }
            Err(reason) => {
                self.push_event(Event::TxRejected { reason });
                false
            }
        }
    }

    /// Bitcoin's reorganisation rule: payments from the blocks that left go back through the same
    /// check as a new payment, ahead of the pool because a waiting payment may spend their
    /// outputs. Whatever the new chain made impossible is thrown out.
    pub(crate) fn settle_pool(&mut self, returning: Vec<Tx>, confirmed: &BTreeSet<Hash>) {
        let back: BTreeSet<Hash> = returning.iter().map(Tx::id).collect();
        let waiting = core::mem::take(&mut self.pool)
            .into_iter()
            .filter(|tx| !confirmed.contains(&tx.id()) && !back.contains(&tx.id()));
        let (kept, refused) = self
            .chain
            .recheck(returning.iter().cloned().chain(waiting).collect());
        // A block may carry what this node would not pass on. It stays out once it leaves the block.
        let light = |tx: &Tx| self.chain.signature_checks(tx) <= MOST_SIGNATURE_CHECKS;
        let (mut kept, heavy): (Vec<Tx>, Vec<Tx>) = kept.into_iter().partition(light);
        let over = kept.split_off(kept.len().min(MAX_POOL));
        for tx in kept.iter().filter(|tx| back.contains(&tx.id())) {
            self.push_event(Event::TxReturned { id: tx.id() });
            self.outbox.push_back(Outgoing::All {
                except: None,
                message: Message::Tx(tx.clone()),
            });
        }
        for (tx, reason) in refused {
            let (id, reason) = (tx.id(), Some(reason));
            self.push_event(Event::TxDropped { id, reason });
        }
        for tx in over.into_iter().chain(heavy) {
            let (id, reason) = (tx.id(), None);
            self.push_event(Event::TxDropped { id, reason });
        }
        self.pool = kept;
    }

    /// A waiting payment may spend the output of one before it, as after a reorganisation.
    pub(crate) fn drop_spent(&mut self) {
        let chain = &self.chain;
        let mut made = BTreeSet::new();
        self.pool.retain(|tx| {
            let spendable = tx
                .inputs
                .iter()
                .all(|input| chain.is_unspent(&input.previous) || made.contains(&input.previous));
            if spendable {
                let txid = tx.id();
                made.extend((0..tx.outputs.len() as u32).map(|index| OutPoint { txid, index }));
            }
            spendable
        });
    }
}
