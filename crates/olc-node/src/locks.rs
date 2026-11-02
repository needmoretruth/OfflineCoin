//! Coins under a beta lock, from this node's side. Every step goes through the check each node
//! runs on a transaction, so a person hears here what the network would say.
use crate::engine::Engine;
use alloc::string::String;
use alloc::vec::Vec;
use olc_core::{Address, ChainError, Hash, OutPoint};
use olc_lang::words::{lock_from_text, words_of, Cost, WordError};
use olc_lang::BETA;
use olc_wallet::locks::{LockError, LockSpend};
use olc_wallet::WalletError;

/// Locked coins a node lists. A list is for reading, and this is about what a person reads.
const MOST_LISTED: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LockFailure {
    Lock(LockError),
    /// No coin under a lock at this place.
    NoCoin,
    /// A node without blocks cannot read locks.
    NeedsBlocks,
    Refused(ChainError),
}

impl From<LockError> for LockFailure {
    fn from(error: LockError) -> Self {
        Self::Lock(error)
    }
}

impl From<WordError> for LockFailure {
    fn from(error: WordError) -> Self {
        Self::Lock(LockError::Words(error))
    }
}

impl From<WalletError> for LockFailure {
    fn from(error: WalletError) -> Self {
        Self::Lock(LockError::Wallet(error))
    }
}

/// A coin under a lock, in words.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LockedCoin {
    pub point: OutPoint,
    pub value: u64,
    pub words: String,
    pub height: u64,
}

impl Engine {
    /// Puts `amount` from this wallet under the lock these words make.
    pub fn pay_to_lock(&mut self, words: &str, amount: u64) -> Result<(Hash, Cost), LockFailure> {
        let (lock, cost) = lock_from_text(words)?;
        let reserved = self.reserved();
        let fee = self.rules().min_fee;
        let tx = self.wallet().pay_lock(self.coins(), &lock, amount, fee, &reserved)?;
        Ok((self.send_signed(tx), cost))
    }

    /// Starts a spend of the coin at `point`, with the signatures this wallet can make.
    pub fn spend_lock(
        &mut self,
        point: OutPoint,
        to: Address,
        unlock: &str,
    ) -> Result<LockSpend, LockFailure> {
        if self.light.is_some() {
            return Err(LockFailure::NeedsBlocks);
        }
        let coin = self.chain.coin_at(&point).filter(|coin| coin.output.language == BETA);
        let coin = coin.ok_or(LockFailure::NoCoin)?;
        let fee = self.rules().min_fee;
        let mut spend = LockSpend::new(point, &coin.output, to, fee, unlock)?;
        spend.fill(self.wallet());
        Ok(spend)
    }

    /// Adds this wallet's signatures to a spend that came as text.
    pub fn sign_lock(&self, text: &str) -> Result<LockSpend, LockFailure> {
        let mut spend = LockSpend::from_text(text)?;
        spend.fill(self.wallet());
        Ok(spend)
    }

    /// Sends a finished spend, once this node's own check passes.
    pub fn finish_lock(&mut self, spend: LockSpend) -> Result<Hash, LockFailure> {
        let tx = spend.finish()?;
        let next = self.chain.height() + 1;
        self.chain.check_tx(&tx, next).map_err(LockFailure::Refused)?;
        Ok(self.send_signed(tx))
    }

    /// Coins under a lock, with their words, oldest first.
    pub fn locked_coins(&self) -> Vec<LockedCoin> {
        let rows = self.chain.coins_under(BETA).take(MOST_LISTED).map(|(point, entry)| LockedCoin {
            point: *point,
            value: entry.output.value,
            words: words_of(&entry.output.lock),
            height: entry.height,
        });
        rows.collect()
    }
}
