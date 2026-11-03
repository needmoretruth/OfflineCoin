//! Wallets shared with other people, from this node's side: their coins, and spends that pass
//! from signer to signer until enough have signed.
use crate::engine::Engine;
use alloc::vec::Vec;
use olc_core::{Address, ChainError, Hash, OutPoint};
use olc_lang::GAMMA;
use olc_wallet::shared::{Shared, SharedError, SharedSpend};

/// Where a shared spend stands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SharedDone {
    Sent(Hash),
    /// Waits for more signatures; the text carries it to the others.
    Waiting(SharedSpend),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SharedFailure {
    Shared(SharedError),
    /// A node holding headers only cannot see the coins.
    NeedsBlocks,
    /// Shared wallets are gamma locks, which the coin takes from its upgrade.
    NotYet,
    Refused(ChainError),
}

impl From<SharedError> for SharedFailure {
    fn from(error: SharedError) -> Self {
        Self::Shared(error)
    }
}

impl Engine {
    /// The coins of a shared wallet that can be spent now, oldest first, with their values.
    pub fn shared_coins(&self, shared: &Shared) -> Vec<(OutPoint, u64)> {
        let lock = shared.lock();
        let next = self.chain.height() + 1;
        let reserved = self.reserved();
        let mut coins: Vec<_> = self
            .chain
            .coins_under(GAMMA)
            .filter(|(point, entry)| entry.output.lock == lock && self.chain.is_mature(entry, next) && !reserved.contains(point))
            .map(|(point, entry)| (entry.height, *point, entry.output.value))
            .collect();
        coins.sort();
        coins.into_iter().map(|(_, point, value)| (point, value)).collect()
    }

    /// Starts a payment from a shared wallet, signed with this wallet's keys in it. Sent at once
    /// when that is enough.
    pub fn pay_shared(&mut self, shared: &Shared, to: Address, amount: u64) -> Result<SharedDone, SharedFailure> {
        if self.light.is_some() {
            return Err(SharedFailure::NeedsBlocks);
        }
        let signing = self.paying_signing().ok_or(SharedFailure::NotYet)?;
        let fee = self.rules().min_fee;
        let coins = self.shared_coins(shared);
        let mut spend = SharedSpend::new(shared, &coins, to, amount, fee, Some(signing))?;
        spend.fill(&self.wallet);
        self.go_on_shared(spend)
    }

    /// Adds this wallet's signatures to a shared spend someone passed on.
    pub fn sign_shared(&mut self, text: &str) -> Result<SharedDone, SharedFailure> {
        let mut spend = SharedSpend::from_text(text)?;
        spend.fill(&self.wallet);
        self.go_on_shared(spend)
    }

    fn go_on_shared(&mut self, spend: SharedSpend) -> Result<SharedDone, SharedFailure> {
        if spend.missing() > 0 {
            return Ok(SharedDone::Waiting(spend));
        }
        let tx = spend.finish()?;
        self.chain.check_tx(&tx, self.chain.height() + 1).map_err(SharedFailure::Refused)?;
        self.send_payment(tx).map_err(|_| SharedFailure::Refused(ChainError::DuplicateInput)).map(SharedDone::Sent)
    }
}
