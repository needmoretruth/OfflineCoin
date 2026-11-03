//! What a payment channel asks of the node it lives in: a funding payment signed but held back
//! until the other side has signed the first state, and transactions it built sent like any other.
use crate::engine::Engine;
use crate::spend::Paying;
use alloc::vec::Vec;
use olc_core::chain::ChainError;
use olc_core::{Address, Coins, Hash, OutPoint, Tx, Unspent};
use olc_wallet::WalletError;

/// A funding lock is two keys and a check, about this long. Only the coins picked depend on it.
const FUNDING_LOCK: usize = 71;

/// The wallet's coins narrowed to the ones a channel holds.
struct Only<'a> {
    coins: Paying<'a>,
    from: &'a [OutPoint],
}

impl Coins for Only<'_> {
    fn height(&self) -> u64 {
        self.coins.height()
    }

    fn unspent_of(&self, address: &Address) -> Vec<(OutPoint, Unspent)> {
        let mut coins = self.coins.unspent_of(address);
        coins.retain(|(point, _)| self.from.contains(point));
        coins
    }

    fn is_mature(&self, entry: &Unspent, spend_height: u64) -> bool {
        self.coins.is_mature(entry, spend_height)
    }

    fn signing_id(&self, height: u64) -> Option<Hash> {
        self.coins.signing_id(height)
    }
}

impl Engine {
    /// Holds `coins` for channels being opened: every other payment leaves them alone. The
    /// channel node says which each turn, from its own files, so a restart holds them again.
    pub fn hold_coins(&mut self, coins: Vec<OutPoint>) {
        self.held = coins;
    }

    /// The coins a funding payment of `amount` would spend now, none held or already spent, and
    /// all in blocks: change still waiting could be taken away by a faster payment in its place,
    /// which would leave the channel without its funding (LND funds from confirmed coins too).
    pub fn pick_funding(&self, amount: u64, fee: u64) -> Result<Vec<OutPoint>, WalletError> {
        let reserved = self.reserved();
        let lock = [0u8; FUNDING_LOCK];
        let tx = self.wallet.pay_lock_in(&self.paying().settled(), olc_lang::GAMMA, &lock, amount, fee, &reserved)?;
        Ok(tx.inputs.iter().map(|input| input.previous).collect())
    }

    /// Signs a payment of `amount` to `lock` in gamma from exactly the coins `from`, and does
    /// not send it.
    pub fn sign_funding(&self, lock: &[u8], amount: u64, fee: u64, from: &[OutPoint]) -> Result<Tx, WalletError> {
        let only = Only { coins: self.paying().settled(), from };
        self.wallet.pay_lock_in(&only, olc_lang::GAMMA, lock, amount, fee, &self.in_flight())
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
