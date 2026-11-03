//! Moving every coin of some keys to another address of this wallet in one payment, as a wallet
//! made before seed words does once, so the words alone bring all its coins back.
use crate::engine::Engine;
use olc_core::{Address, Hash};
use olc_wallet::WalletError;

/// The most coins one move takes, so the payment stays well inside a block. A move with more
/// left over is done again.
pub const MOST_MOVED: usize = 200;

impl Engine {
    /// Pays every mature coin of `from` to `to`, less the fee. Gives the payment and what arrives.
    pub fn move_coins(&mut self, from: &[Address], to: Address) -> Result<(Hash, u64), WalletError> {
        let reserved = self.reserved();
        let fee = self.rules().min_fee;
        let coin = self.paying_signing();
        let (unsigned, value) =
            self.wallet.sweep(&self.chain, from, to, fee, &reserved, MOST_MOVED, coin).ok_or(WalletError::NotEnoughCoins)?;
        let tx = self.wallet.sign(&unsigned)?;
        let id = self.send_payment(tx)?;
        Ok((id, value))
    }
}
