//! Coins under a beta or gamma lock, from this node's side. From its upgrade a coin writes new
//! locks in gamma, today's Bitcoin Script, and keeps spending beta locks made before. Every step
//! goes through the check each node runs on a transaction, so a person hears here what the network
//! would say.
use crate::engine::Engine;
use alloc::string::String;
use alloc::vec::Vec;
use olc_core::{Address, ChainError, Hash, OutPoint};
use olc_lang::words::{lock_from_text_in, unlock_for_in, words_of_in, Cost, Dialect, WordError};
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
    /// The unlock words a preset's lock takes, empty for any other lock.
    pub unlock: String,
    pub height: u64,
}

impl Engine {
    /// Puts `amount` from this wallet under the lock these words make.
    pub fn pay_to_lock(&mut self, words: &str, amount: u64) -> Result<(Hash, Cost), LockFailure> {
        let dialect = self.lock_dialect();
        let (lock, cost) = lock_from_text_in(dialect, words)?;
        let reserved = self.reserved();
        let fee = self.rules().min_fee;
        let wallet = self.wallet();
        let tx = wallet.pay_lock_in(&self.paying(), dialect.language(), &lock, amount, fee, &reserved)?;
        Ok((self.send_payment(tx)?, cost))
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
        let coin = self.chain.coin_at(&point).filter(|coin| Dialect::of(coin.output.language).is_some());
        let coin = coin.ok_or(LockFailure::NoCoin)?;
        let fee = self.rules().min_fee;
        let signing = self.paying_signing();
        let mut spend = LockSpend::new(point, &coin.output, to, fee, unlock, signing)?;
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
        Ok(self.send_payment(tx)?)
    }

    /// The words a new lock is written in: gamma once version 2 payments are signed here, beta for
    /// version 1 ones.
    pub fn lock_dialect(&self) -> Dialect {
        match self.paying_signing() {
            Some(_) => Dialect::Gamma,
            None => Dialect::Beta,
        }
    }

    /// Coins under a lock, with their words, oldest first.
    pub fn locked_coins(&self) -> Vec<LockedCoin> {
        let under = |dialect: Dialect| self.chain.coins_under(dialect.language()).map(move |row| (dialect, row));
        let mut rows: Vec<_> = under(Dialect::Beta).chain(under(Dialect::Gamma)).collect();
        rows.sort_by_key(|(_, (_, entry))| entry.height);
        let rows = rows.into_iter().take(MOST_LISTED).map(|(dialect, (point, entry))| LockedCoin {
            point: *point,
            value: entry.output.value,
            words: words_of_in(dialect, &entry.output.lock),
            unlock: unlock_for_in(dialect, &entry.output.lock),
            height: entry.height,
        });
        rows.collect()
    }
}
