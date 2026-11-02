//! Coins under a beta lock, for a person or for a node's own command line.
use super::commands::LockOrder;
use super::host::{Host, PayError, Waiting};
use super::host_parts::unix_ms;
use super::{format_coins, parse_coins};
use olc_core::{ChainError, Hash, OutPoint};
use olc_crypto::hash;
use olc_lang::words::{
    hash_lock, lock_from_text, public_key_from_text, time_lock, two_of_two, Cost, WordError,
};
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

/// A lock these words open.
pub fn hash_lock_for(secret: &str) -> String {
    hash_lock(&hash(secret.as_bytes())[..20])
}

/// A lock that needs both of these public keys, given as hex.
pub fn two_of_two_for(first: &str, second: &str) -> Option<String> {
    Some(two_of_two(&public_key_from_text(first)?, &public_key_from_text(second)?))
}

/// A lock this address opens from `height` on.
pub fn time_lock_for(height: &str, address: &str) -> Option<String> {
    Some(time_lock(height.trim().parse().ok()?, &parse_address(address.trim())?))
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
        WordError::TooManyChecks(count) => ("lock.too_many_checks", count.to_string()),
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
        LockFailure::Lock(LockError::Wallet(WalletError::Refused)) => {
            ("notice.pay_refused", String::new())
        }
        LockFailure::Lock(LockError::Wallet(WalletError::ZeroAmount)) => {
            ("lock.too_small", String::new())
        }
        LockFailure::Lock(LockError::Wallet(_)) => ("lock.no_key", String::new()),
        LockFailure::NoCoin => ("lock.no_coin", String::new()),
        LockFailure::NeedsBlocks => ("lock.needs_blocks", String::new()),
        LockFailure::Refused(ChainError::BadLock) => ("lock.not_open", String::new()),
        LockFailure::Refused(ChainError::FeeTooLow | ChainError::Overspend) => {
            ("lock.too_small", String::new())
        }
        LockFailure::Refused(ChainError::MissingInput) => ("lock.spent", String::new()),
        LockFailure::Refused(ChainError::ImmatureCoinbase) => ("lock.immature", String::new()),
        LockFailure::Refused(_) => ("lock.not_taken", String::new()),
    }
}

/// Every way a wallet says no, for reading its name back from a detail written before.
const WALLET_ERRORS: [WalletError; 6] = [
    WalletError::BadKey,
    WalletError::NotEnoughCoins,
    WalletError::ZeroAmount,
    WalletError::NoSuchKey,
    WalletError::KnownKey,
    WalletError::Refused,
];

/// Why a wallet said no, as a sentence.
pub fn wallet_words(error: &WalletError) -> &'static str {
    match error {
        WalletError::BadKey => "That key does not read right.",
        WalletError::NotEnoughCoins => "There are not enough coins for that.",
        WalletError::ZeroAmount => "The amount has to be more than zero.",
        WalletError::NoSuchKey => "This wallet has no such key.",
        WalletError::KnownKey => "This wallet already holds that key.",
        WalletError::Refused => "The payment could not join the queue. Send less, or try after the next block.",
    }
}

/// A detail as a person reads it. Some orders still give a reason by its name in the code: a
/// wallet's reason reads as its sentence, and any other such name is left out.
pub fn plain_detail(detail: &str) -> &str {
    let named = WALLET_ERRORS.iter().find(|error| {
        let inside = format!("{:?}", PayError::Wallet(**error));
        format!("{error:?}") == detail || inside == detail
    });
    match named {
        Some(error) => wallet_words(error),
        None if reads_as_code(detail) => "",
        None => detail,
    }
}

/// A name such as `Stopped`, `NoPeer` or `Lock(Words(Empty))`: a payment's reason by its name, one
/// word with a capital inside it, or one that opens a bracket.
fn reads_as_code(detail: &str) -> bool {
    let named = [PayError::Stopped, PayError::BadAddress].map(|error| format!("{error:?}"));
    let name: String = detail.chars().take_while(char::is_ascii_alphanumeric).collect();
    let next = detail[name.len()..].chars().next();
    let capital = name.starts_with(|c: char| c.is_ascii_uppercase());
    let inner = name.chars().skip(1).any(|c| c.is_ascii_uppercase()) && next.is_none();
    named.contains(&detail.to_string()) || (capital && (inner || matches!(next, Some('(' | '{'))))
}

fn failed(failure: &LockFailure) -> (&'static str, String, bool) {
    let (key, detail) = fault_of_lock(failure);
    (key, detail, false)
}

impl Host {
    /// Does a lock order and says how it went: a notice's key, its detail, and whether it went well.
    pub fn carry_lock(&self, order: LockOrder) -> (&'static str, String, bool) {
        let done = match order {
            LockOrder::Pay { words, amount } => {
                return match self.pay_lock(&words, &amount) {
                    Ok((id, amount)) => {
                        let coin = OutPoint { txid: id, index: 0 };
                        ("lock.paid", format!("{amount} {}", name_of_coin(&coin)), true)
                    }
                    Err(failure) => failed(&failure),
                };
            }
            LockOrder::Spend { coin, to, unlock } => self.spend_lock(&coin, &to, &unlock),
            LockOrder::Sign { text } => self.sign_lock(&text),
        };
        match done {
            Ok(LockDone::Sent(id)) => {
                ("lock.sent", id.iter().map(|byte| format!("{byte:02x}")).collect(), true)
            }
            Ok(LockDone::Waiting { text, .. }) => ("lock.waiting", text, true),
            Err(failure) => failed(&failure),
        }
    }

    /// Pays at the lowest fee the coin allows.
    pub fn pay(&self, to: &str, amount: u64) -> Result<Hash, PayError> {
        let to = parse_address(to.trim()).ok_or(PayError::BadAddress)?;
        let mut runner = self.ready().map_err(|_| PayError::Stopped)?;
        let fee = runner.engine.rules().min_fee;
        let id = runner.engine.pay(to, amount, fee).map_err(PayError::Wallet)?;
        if let Ok(mut waiting) = self.state.waiting.lock() {
            let time_ms = unix_ms();
            waiting.push(Waiting {
                id,
                amount,
                time_ms,
            });
        }
        Ok(id)
    }

    /// Puts coins from this wallet under the lock these words make.
    pub fn pay_lock(&self, words: &str, amount: &str) -> Result<(Hash, String), LockFailure> {
        let units = parse_coins(amount).filter(|units| *units > 0);
        let units = units.ok_or(LockFailure::Lock(LockError::TooSmall))?;
        let mut runner = self.ready().map_err(|_| LockFailure::NoCoin)?;
        let (id, _) = runner.engine.pay_to_lock(words, units)?;
        Ok((id, format_coins(units)))
    }

    /// Spends a locked coin to `to`, or to this wallet when `to` is empty.
    pub fn spend_lock(&self, coin: &str, to: &str, unlock: &str) -> Result<LockDone, LockFailure> {
        let point = coin_named(coin).ok_or(LockFailure::NoCoin)?;
        let mut runner = self.ready().map_err(|_| LockFailure::NoCoin)?;
        let to = match to.trim() {
            "" => runner.engine.wallet().address(),
            to => parse_address(to).ok_or(LockFailure::Lock(LockError::BadText))?,
        };
        let spend = runner.engine.spend_lock(point, to, unlock)?;
        go_on(&mut runner.engine, spend)
    }

    /// Adds this wallet's signatures to a spend that came as text.
    pub fn sign_lock(&self, text: &str) -> Result<LockDone, LockFailure> {
        let mut runner = self.ready().map_err(|_| LockFailure::NoCoin)?;
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

    #[test]
    fn a_lock_that_does_not_open_says_so_in_words() {
        let (key, detail) = fault_of_lock(&LockFailure::Refused(ChainError::BadLock));
        assert_eq!((key, detail.as_str()), ("lock.not_open", ""));
        let (key, _) = fault_of_lock(&LockFailure::Refused(ChainError::MissingInput));
        assert_eq!(key, "lock.spent");
    }

    #[test]
    fn no_lock_failure_carries_a_name_from_the_code() {
        let failures = WALLET_ERRORS.map(|error| LockFailure::Lock(LockError::Wallet(error)));
        let refusals = [ChainError::ImmatureCoinbase, ChainError::TooLarge, ChainError::AlreadyKnown];
        let all = failures.into_iter().chain(refusals.map(LockFailure::Refused));
        for failure in all {
            let (key, detail) = fault_of_lock(&failure);
            assert!(detail.is_empty(), "{key} carries {detail}");
        }
    }

    #[test]
    fn a_wallet_reason_given_by_name_reads_as_a_sentence() {
        for error in WALLET_ERRORS {
            let named = format!("{error:?}");
            assert_eq!(plain_detail(&named), wallet_words(&error));
        }
        assert_eq!(plain_detail("none.json: not found"), "none.json: not found");
        let from_a_node = format!("{:?}", PayError::Wallet(WalletError::NotEnoughCoins));
        assert_eq!(plain_detail(&from_a_node), wallet_words(&WalletError::NotEnoughCoins));
        let lock = format!("{:?}", LockFailure::Lock(LockError::Words(WordError::Unknown("x y".into()))));
        assert_eq!(plain_detail(&lock), "");
        assert_eq!(plain_detail(&format!("{:?}", PayError::Stopped)), "");
        assert_eq!(plain_detail("No peer to take it from"), "No peer to take it from");
        assert_eq!(plain_detail("Test"), "Test");
    }
}
