//! Coins under a beta lock, for a person or for a node's own command line.
use super::host::Host;
use super::{format_coins, parse_coins};
use olc_core::{Hash, OutPoint};
use olc_lang::words::{lock_from_text, Cost, WordError};
use olc_node::locks::{LockFailure, LockedCoin};
use olc_wallet::locks::{LockError, LockSpend};
use olc_wallet::{format_address, parse_address, WalletError};

/// What a spend came to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LockDone {
    Sent(Hash),
    /// Waits for these addresses to sign. The text carries it to them.
    Waiting { text: String, who: Vec<String> },
}

/// A coin as a person names it, by its payment and place.
pub fn name_of_coin(point: &OutPoint) -> String {
    let id: String = point.txid.iter().map(|byte| format!("{byte:02x}")).collect();
    format!("{id}:{}", point.index)
}

fn coin_named(text: &str) -> Option<OutPoint> {
    let (id, index) = text.trim().rsplit_once(':')?;
    let txid = crate::core::explorer::parse_hash(id)?;
    Some(OutPoint { txid, index: index.parse().ok()? })
}

/// What the words of a lock cost, or what is wrong with them.
pub fn check_lock(words: &str) -> Result<Cost, WordError> {
    lock_from_text(words).map(|(_, cost)| cost)
}

/// The words of a lock's fault, as the key of a sentence and what to put in it.
pub fn fault_of_words(error: &WordError) -> (&'static str, String) {
    match error {
        WordError::Empty => ("lock.no_words", String::new()),
        WordError::Unknown(word) => ("lock.unknown_word", word.clone()),
        WordError::TooLong(word) => ("lock.too_long_word", word.clone()),
        WordError::OpenQuote => ("lock.open_quote", String::new()),
        WordError::OpenBranch => ("lock.open_branch", String::new()),
        WordError::TooManyOps(count) => ("lock.too_many_ops", count.to_string()),
        WordError::TooManyBytes(count) => ("lock.too_many_bytes", count.to_string()),
    }
}

/// A failure as the key of a sentence and what to put in it.
pub fn fault_of_lock(failure: &LockFailure) -> (&'static str, String) {
    match failure {
        LockFailure::Lock(LockError::Words(error)) => fault_of_words(error),
        LockFailure::Lock(LockError::TooSmall) => ("lock.too_small", String::new()),
        LockFailure::Lock(LockError::BadText) => ("lock.bad_text", String::new()),
        LockFailure::Lock(LockError::Wallet(WalletError::NotEnoughCoins)) => {
            ("notice.not_enough", String::new())
        }
        LockFailure::Lock(LockError::Wallet(error)) => ("lock.wallet", format!("{error:?}")),
        LockFailure::NoCoin => ("lock.no_coin", String::new()),
        LockFailure::NeedsBlocks => ("lock.needs_blocks", String::new()),
        LockFailure::Refused(error) => ("lock.refused", format!("{error:?}")),
    }
}

impl Host {
    /// Puts coins from this wallet under the lock these words make.
    pub fn pay_lock(&self, words: &str, amount: &str) -> Result<(Hash, String), LockFailure> {
        let units = parse_coins(amount).filter(|units| *units > 0);
        let units = units.ok_or(LockFailure::Lock(LockError::TooSmall))?;
        let mut runner = self.runner.lock().map_err(|_| LockFailure::NoCoin)?;
        let (id, _) = runner.engine.pay_to_lock(words, units)?;
        Ok((id, format_coins(units)))
    }

    /// Spends a locked coin to `to`, or to this wallet when `to` is empty.
    pub fn spend_lock(&self, coin: &str, to: &str, unlock: &str) -> Result<LockDone, LockFailure> {
        let point = coin_named(coin).ok_or(LockFailure::NoCoin)?;
        let mut runner = self.runner.lock().map_err(|_| LockFailure::NoCoin)?;
        let to = match to.trim() {
            "" => runner.engine.wallet().address(),
            to => parse_address(to).ok_or(LockFailure::Lock(LockError::BadText))?,
        };
        let spend = runner.engine.spend_lock(point, to, unlock)?;
        go_on(&mut runner.engine, spend)
    }

    /// Adds this wallet's signatures to a spend that came as text.
    pub fn sign_lock(&self, text: &str) -> Result<LockDone, LockFailure> {
        let mut runner = self.runner.lock().map_err(|_| LockFailure::NoCoin)?;
        let spend = runner.engine.sign_lock(text)?;
        go_on(&mut runner.engine, spend)
    }

    /// Coins under a lock, as this node sees them.
    pub fn locked_coins(&self) -> Vec<LockedCoin> {
        self.runner.lock().map_or_else(|_| Vec::new(), |runner| runner.engine.locked_coins())
    }

    /// This wallet's address, for a spend that pays it.
    pub fn own_address(&self) -> Option<String> {
        let runner = self.runner.lock().ok()?;
        Some(format_address(&runner.engine.wallet().address()))
    }
}

fn go_on(engine: &mut olc_node::Engine, spend: LockSpend) -> Result<LockDone, LockFailure> {
    let who = spend.waiting_for();
    if who.is_empty() {
        return Ok(LockDone::Sent(engine.finish_lock(spend)?));
    }
    Ok(LockDone::Waiting { text: spend.to_text(), who })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_coin_reads_back_from_its_name() {
        let point = OutPoint { txid: [7; 32], index: 3 };
        assert_eq!(coin_named(&name_of_coin(&point)), Some(point));
        assert_eq!(coin_named("nothing"), None);
    }
}
