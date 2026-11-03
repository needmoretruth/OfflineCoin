//! What a payment channel asks of the node it lives in: a funding payment signed but held back
//! until the other side has signed the first state, and transactions it built sent like any other.
use crate::engine::Engine;
use olc_core::chain::ChainError;
use olc_core::{Hash, Tx};
use olc_wallet::WalletError;

impl Engine {
    /// Signs a payment of `amount` from the wallet to `lock` in gamma, and does not send it.
    pub fn sign_funding(&self, lock: &[u8], amount: u64, fee: u64) -> Result<Tx, WalletError> {
        let reserved = self.reserved();
        self.wallet.pay_lock_in(&self.paying(), olc_lang::GAMMA, lock, amount, fee, &reserved)
    }

    /// Checks a transaction as the next block would and sends it.
    pub fn publish(&mut self, tx: Tx) -> Result<Hash, ChainError> {
        let id = tx.id();
        if self.in_pool(&id) {
            return Ok(id);
        }
        self.chain.check_tx(&tx, self.chain.height() + 1)?;
        self.send_own(tx);
        Ok(id)
    }
}
