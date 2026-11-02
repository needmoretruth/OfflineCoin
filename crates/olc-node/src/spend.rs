//! Coins leaving this node's own wallet. Each one is an ordinary transaction.
use crate::engine::Engine;
use crate::event::Event;
use alloc::vec::Vec;
use olc_core::{Address, Hash, OutPoint, Tx};
use olc_wallet::WalletError;

/// Own payments remembered, so their change does not read as money received.
const MAX_SENT: usize = 16;

/// A merge waiting for its signatures. A board takes a good part of a second over each.
pub(crate) struct Signing {
    tx: Tx,
    owners: Vec<Address>,
    done: usize,
}

impl Engine {
    /// Signs a payment from this node's wallet and sends it to the network.
    pub fn pay(&mut self, to: Address, amount: u64, fee: u64) -> Result<Hash, WalletError> {
        let reserved = self.reserved();
        let tx = self.wallet.pay(self.coins(), to, amount, fee, &reserved)?;
        let id = self.send_payment(tx)?;
        self.push_event(Event::PaymentSent { amount });
        Ok(id)
    }

    /// Pays many addresses in one transaction.
    pub fn pay_many(&mut self, payments: &[(Address, u64)], fee: u64) -> Result<Hash, WalletError> {
        let reserved = self.reserved();
        let tx = self.wallet.pay_many(self.coins(), payments, fee, &reserved)?;
        let amount = payments.iter().map(|(_, amount)| *amount).fold(0, u64::saturating_add);
        let id = self.send_payment(tx)?;
        self.push_event(Event::PaymentSent { amount });
        Ok(id)
    }

    /// Writes `data` into the chain with a transaction of its own. The fee is the coin's lowest.
    pub fn write_note(&mut self, data: &[u8]) -> Result<Hash, WalletError> {
        let reserved = self.reserved();
        let fee = self.chain.rules().min_fee;
        let tx = self.wallet.note(self.coins(), data, fee, &reserved)?;
        self.send_payment(tx)
    }

    /// Sends a payment this node signed. A person hears when its own pool would not take it,
    /// because nobody else would ever see it then.
    fn send_payment(&mut self, tx: Tx) -> Result<Hash, WalletError> {
        let id = self.send_signed(tx);
        match self.light.is_some() || self.in_pool(&id) {
            true => Ok(id),
            false => Err(WalletError::Refused),
        }
    }

    fn reserved(&self) -> Vec<OutPoint> {
        let signing = self.signing.iter().map(|signing| &signing.tx);
        let inputs = self.pool.iter().chain(signing).flat_map(|tx| &tx.inputs);
        inputs.map(|input| input.previous).collect()
    }

    pub(crate) fn send_own(&mut self, tx: Tx) -> Hash {
        let id = tx.id();
        self.sent.push_back(id);
        if self.sent.len() > MAX_SENT {
            self.sent.pop_front();
        }
        self.receive_tx(None, tx);
        id
    }

    /// Merges small coins when a key has gathered many.
    pub(crate) fn merge_coins(&mut self) {
        // While catching up, coins that look unspent may already be spent further on.
        if !self.merging || self.signing.is_some() || self.syncing_to().is_some() {
            return;
        }
        let reserved = self.reserved();
        let fee = self.chain.rules().min_fee;
        if let Some((tx, owners)) = self.wallet.merge(&self.chain, fee, &reserved) {
            self.signing = Some(Signing { tx, owners, done: 0 });
        }
    }

    /// One signature of the merge under way, then back to mining and talking.
    pub(crate) fn sign_a_little(&mut self) {
        let Some(mut signing) = self.signing.take() else {
            return;
        };
        let done = signing.done;
        if self.wallet.sign_input(&mut signing.tx, &signing.owners, done).is_err() {
            return;
        }
        signing.done += 1;
        if signing.done < signing.owners.len() {
            self.signing = Some(signing);
            return;
        }
        self.send_own(signing.tx);
        self.push_event(Event::CoinsMerged { inputs: done + 1 });
    }
}
