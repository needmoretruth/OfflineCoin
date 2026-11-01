//! Coins leaving this node's own wallet. Each one is an ordinary transaction.
use crate::engine::Engine;
use crate::event::Event;
use alloc::vec::Vec;
use olc_core::{Address, Hash, OutPoint, Tx};
use olc_wallet::WalletError;

/// Own payments remembered, so their change does not read as money received.
const MAX_SENT: usize = 16;

impl Engine {
    /// Signs a payment from this node's wallet and sends it to the network.
    pub fn pay(&mut self, to: Address, amount: u64, fee: u64) -> Result<Hash, WalletError> {
        let reserved = self.reserved();
        let tx = self.wallet.pay(self.coins(), to, amount, fee, &reserved)?;
        self.push_event(Event::PaymentSent { amount });
        Ok(self.send_signed(tx))
    }

    /// Pays many addresses in one transaction.
    pub fn pay_many(&mut self, payments: &[(Address, u64)], fee: u64) -> Result<Hash, WalletError> {
        let reserved = self.reserved();
        let tx = self.wallet.pay_many(self.coins(), payments, fee, &reserved)?;
        let amount = payments.iter().map(|(_, amount)| *amount).sum();
        self.push_event(Event::PaymentSent { amount });
        Ok(self.send_signed(tx))
    }

    /// Writes `data` into the chain with a transaction of its own. The fee is the coin's lowest.
    pub fn write_note(&mut self, data: &[u8]) -> Result<Hash, WalletError> {
        let reserved = self.reserved();
        let fee = self.chain.rules().min_fee;
        let tx = self.wallet.note(self.coins(), data, fee, &reserved)?;
        Ok(self.send_signed(tx))
    }

    fn reserved(&self) -> Vec<OutPoint> {
        let inputs = self.pool.iter().flat_map(|tx| &tx.inputs);
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
        if !self.merging {
            return;
        }
        let reserved = self.reserved();
        let fee = self.chain.rules().min_fee;
        if let Some(tx) = self.wallet.merge(&self.chain, fee, &reserved) {
            let inputs = tx.inputs.len();
            self.send_own(tx);
            self.push_event(Event::CoinsMerged { inputs });
        }
    }
}
