//! The pool: payments that wait for a block. What it takes comes from anyone in reach, so it has a
//! limit in bytes as well as in payments, and the cheap checks come before the signature.
use crate::engine::{Engine, PeerId};
use crate::event::{Event, Outgoing};
use crate::message::Message;
use alloc::collections::BTreeSet;
use alloc::vec::Vec;
use olc_core::{ChainError, Hash, OutPoint, Tx};
use olc_lang::words::MOST_SIGNATURE_CHECKS;

const MAX_POOL: usize = 256;
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

    /// Why the pool has no place for `tx`, whatever the chain says about it. These checks come
    /// first because a signature costs a board far more than all of them.
    fn pool_refuses(&self, tx: &Tx) -> Option<ChainError> {
        let size = tx.encode().len();
        if !self.chain.fits_a_block(size) {
            return Some(ChainError::TooLarge);
        }
        // The first spend of a coin stays, as in Bitcoin: two can never both confirm, and the
        // second would only take room from payments that can.
        let mut taken = self.pool.iter().flat_map(|known| &known.inputs);
        if taken.any(|input| tx.inputs.iter().any(|new| new.previous == input.previous)) {
            return Some(ChainError::DuplicateInput);
        }
        let held: usize = self.pool.iter().map(|known| known.encode().len()).sum();
        (held.saturating_add(size) > MOST_POOL_BYTES).then_some(ChainError::TooLarge)
    }

    pub(crate) fn receive_tx(&mut self, from: Option<PeerId>, tx: Tx) -> bool {
        if tx.is_coinbase() || self.pool.iter().any(|known| known == &tx) {
            return false;
        }
        if self.pool.len() >= MAX_POOL {
            return false;
        }
        if tx.outputs.iter().any(|out| out.is_data() && out.lock.len() > MOST_DATA) {
            self.push_event(Event::DataRefused);
            return false;
        }
        if let Some(reason) = self.pool_refuses(&tx) {
            self.push_event(Event::TxRejected { reason });
            return false;
        }
        let checks = self.chain.signature_checks(&tx);
        if checks > MOST_SIGNATURE_CHECKS {
            self.push_event(Event::ChecksRefused { checks });
            return false;
        }
        match self.chain.check_tx(&tx, self.chain.height() + 1) {
            Ok(_) => {
                self.push_event(Event::TxAccepted);
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
