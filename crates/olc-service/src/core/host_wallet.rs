//! The wallet's newer parts on a running node: seed words to write down, shared wallets with what
//! they hold, spends of them that pass between signers, and speeding up a waiting payment.
use super::explorer::parse_hash;
use super::history::{Activity, ActivityKind};
use super::host::Host;
use super::host_locks::wallet_key;
use olc_core::amount::{format_coins, parse_coins};
use olc_core::{Address, Hash};
use olc_node::shared::{SharedDone, SharedFailure};
use olc_node::Engine;
use olc_wallet::shared::{SharedError, SharedSpend};
use olc_wallet::{parse_address, SeedRecord, Wallet};
use std::collections::BTreeSet;
use std::sync::Mutex;

/// Seed words on their way to the screen. They never show in a log.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Words(pub Vec<String>);

impl std::fmt::Debug for Words {
    fn fmt(&self, out: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(out, "Words({})", self.0.len())
    }
}

/// A shared wallet as the screen shows it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SharedView {
    /// What others pay it with, and what the other signers add.
    pub text: String,
    pub need: usize,
    /// The address of each key, in order.
    pub signers: Vec<String>,
    /// The places of the keys this wallet holds.
    pub mine: Vec<usize>,
    pub balance: u64,
}

/// What speeding up a waiting payment would cost, worked out before the person agrees.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpeedQuote {
    pub id: Hash,
    /// The fee it pays now.
    pub now: u64,
    /// The fee it would pay.
    pub new: u64,
}

/// The wallet beyond its keys.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WalletView {
    /// The seed words while the person has not yet said they wrote them down.
    pub words_to_write: Option<Words>,
    pub has_words: bool,
    /// The words, for showing again once the person asks and agrees.
    pub words: Option<Words>,
    /// Words this wallet had before, which still bring their coins back.
    pub earlier_words: Vec<Words>,
    /// Coins on keys no words can bring back, from before the wallet had words.
    pub outside_words: u64,
    /// The part of it a payment can move now; mined coins wait until they mature.
    pub outside_ripe: u64,
    pub shared: Vec<SharedView>,
    /// The block from which this coin takes shared wallets.
    pub shared_from: u64,
    pub speed: Vec<SpeedQuote>,
}

fn words_of(record: &SeedRecord) -> Option<Words> {
    record.mnemonic().map(|words| Words(words.words()))
}

/// The addresses words name, worked out once for each set of words: deriving them takes a
/// moment, and the view is made several times a second.
fn word_addresses(wallet: &Wallet) -> BTreeSet<Address> {
    /// Each set of words by the hash of its entropy and how many keys it names, and those keys.
    type Named = (Vec<(Hash, u32)>, BTreeSet<Address>);
    static KNOWN: Mutex<Option<Named>> = Mutex::new(None);
    let records = wallet.seed.iter().chain(&wallet.earlier);
    let named: Vec<(Hash, u32)> = records.map(|record| (olc_crypto::hash(&record.entropy), record.next)).collect();
    let Ok(mut known) = KNOWN.lock() else {
        return wallet.word_addresses().into_iter().collect();
    };
    match known.as_ref() {
        Some((was, addresses)) if *was == named => addresses.clone(),
        _ => {
            let addresses: BTreeSet<Address> = wallet.word_addresses().into_iter().collect();
            *known = Some((named, addresses.clone()));
            addresses
        }
    }
}

/// Keys of this wallet that no words bring back. None while the wallet has no words.
pub(super) fn keys_outside_words(wallet: &Wallet) -> Vec<Address> {
    if wallet.seed.is_none() {
        return Vec::new();
    }
    let named = word_addresses(wallet);
    wallet.keys().map(|(address, _)| address).filter(|address| !named.contains(address)).collect()
}

/// An amount as every notice gives it: in coins, with the coin's unit.
pub(super) fn amount_of(engine: &Engine, units: u64) -> String {
    format!("{} {}", format_coins(units), engine.chain().config().ticker)
}

pub(super) fn wallet_view(engine: &Engine, activity: &[Activity]) -> WalletView {
    let wallet = engine.wallet();
    let coins = engine.coins();
    // A coin a waiting payment spends, such as the move to the words, is on its way out.
    let leaving: BTreeSet<_> = engine.pool().iter().flat_map(|tx| tx.inputs.iter().map(|input| input.previous)).collect();
    let next = coins.height() + 1;
    let outside_coins = keys_outside_words(wallet).into_iter().flat_map(|address| coins.unspent_of(&address));
    let outside: Vec<_> = outside_coins.filter(|(point, _)| !leaving.contains(point)).collect();
    let ripe = outside.iter().filter(|(_, entry)| coins.is_mature(entry, next)).map(|(_, entry)| entry.output.value).sum();
    let outside = outside.iter().map(|(_, entry)| entry.output.value).sum();
    let waiting = activity.iter().filter(|item| item.kind == ActivityKind::Sent && item.height.is_none());
    let speed = waiting.filter_map(|item| engine.speed_up_fees(&item.id).ok().map(|(now, new)| SpeedQuote { id: item.id, now, new }));
    let unwritten = wallet.seed.as_ref().filter(|seed| !seed.written);
    let shared = wallet.shared.iter().map(|shared| SharedView {
        text: shared.to_text(),
        need: usize::from(shared.need),
        signers: (0..shared.keys.len()).map(|at| shared.signer(at)).collect(),
        mine: shared.held_by(wallet),
        balance: engine.shared_coins(shared).iter().map(|(_, value)| value).sum(),
    });
    WalletView {
        words_to_write: unwritten.and_then(words_of),
        has_words: wallet.seed.is_some(),
        words: wallet.seed.as_ref().and_then(words_of),
        earlier_words: wallet.earlier.iter().filter_map(words_of).collect(),
        outside_words: outside,
        outside_ripe: ripe,
        shared: shared.collect(),
        shared_from: engine.chain().config().upgrade_from,
        speed: speed.collect(),
    }
}

fn hex(id: &[u8]) -> String {
    id.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn shared_fault(failure: &SharedFailure) -> &'static str {
    match failure {
        SharedFailure::Shared(SharedError::NotEnoughCoins) => "notice.not_enough",
        SharedFailure::Shared(_) => "notice.shared_bad",
        SharedFailure::NeedsBlocks => "notice.shared_refused",
        SharedFailure::NotYet => "notice.shared_not_yet",
        SharedFailure::Refused(_) => "notice.shared_refused",
    }
}

impl Host {
    /// An amount as every notice gives it. See [`amount_of`].
    pub fn amount_text(&self, units: u64) -> String {
        match self.ready() {
            Ok(runner) => amount_of(&runner.engine, units),
            Err(_) => format_coins(units),
        }
    }

    /// Moves the coins on keys from before the words to the words' first key, in one payment.
    pub fn move_to_words(&self) -> (&'static str, String, bool) {
        let Ok(mut runner) = self.ready() else {
            return ("notice.not_enough", String::new(), false);
        };
        let wallet = runner.engine.wallet();
        let from = keys_outside_words(wallet);
        let named = word_addresses(wallet);
        let to = wallet.keys().map(|(address, _)| address).find(|address| named.contains(address));
        let (Some(to), false) = (to, from.is_empty()) else {
            return ("notice.not_enough", String::new(), false);
        };
        match runner.engine.move_coins(&from, to) {
            Ok((_, value)) => ("notice.moved_to_words", amount_of(&runner.engine, value), true),
            Err(error) => (wallet_key(&error), String::new(), false),
        }
    }

    /// The block from which this coin takes shared wallets.
    pub(super) fn shared_from(&self) -> u64 {
        self.ready().map_or(0, |runner| runner.engine.chain().config().upgrade_from)
    }

    /// Whether this coin takes the gamma locks a shared wallet is, so coins sent to one can be
    /// found and spent again.
    pub(super) fn shared_ready(&self) -> bool {
        self.ready().is_ok_and(|runner| runner.engine.lock_dialect() == olc_lang::words::Dialect::Gamma)
    }

    fn shared_went(&self, done: Result<SharedDone, SharedFailure>) -> (&'static str, String, bool) {
        match done {
            Ok(SharedDone::Sent(id)) => {
                self.set_half_spend(None);
                ("notice.shared_sent", hex(&id), true)
            }
            Ok(SharedDone::Waiting(spend)) => {
                let text = spend.to_text();
                self.set_half_spend(Some((text.clone(), waiting_for(&spend))));
                ("notice.shared_waiting", text, true)
            }
            Err(SharedFailure::NotYet) => ("notice.shared_not_yet", self.shared_from().to_string(), false),
            Err(failure) => (shared_fault(&failure), String::new(), false),
        }
    }

    /// Pays `amount` to `to` from shared wallet number `wallet`.
    pub fn pay_shared(&self, wallet: usize, to: &str, amount: &str) -> (&'static str, String, bool) {
        let Some(to) = parse_address(to.trim()) else {
            return ("notice.bad_address", String::new(), false);
        };
        let Some(units) = parse_coins(amount.trim()).filter(|units| *units > 0) else {
            return ("notice.bad_amount", String::new(), false);
        };
        let Ok(mut runner) = self.ready() else {
            return ("notice.shared_refused", String::new(), false);
        };
        let Some(shared) = runner.engine.wallet().shared.get(wallet).cloned() else {
            return ("notice.shared_bad", String::new(), false);
        };
        let done = runner.engine.pay_shared(&shared, to, units);
        drop(runner);
        self.shared_went(done)
    }

    /// Adds this wallet's signatures to a shared spend someone passed on.
    pub fn sign_shared(&self, text: &str) -> (&'static str, String, bool) {
        let Ok(mut runner) = self.ready() else {
            return ("notice.shared_refused", String::new(), false);
        };
        let done = runner.engine.sign_shared(text);
        drop(runner);
        self.shared_went(done)
    }

    /// Speeds up a waiting payment. One sent from here keeps its place in the list under its new id.
    pub fn speed_up(&self, id: &str) -> (&'static str, String, bool) {
        let Some(id) = parse_hash(id.trim()) else {
            return ("notice.not_waiting", String::new(), false);
        };
        let Ok(mut runner) = self.ready() else {
            return ("notice.not_waiting", String::new(), false);
        };
        if !runner.engine.in_pool(&id) {
            return ("notice.not_waiting", String::new(), false);
        }
        match runner.engine.speed_up(&id) {
            Ok(sped) => {
                if let Ok(mut waiting) = self.state.waiting.lock() {
                    waiting.iter_mut().filter(|item| item.id == id && sped.replaced).for_each(|item| item.id = sped.id);
                }
                ("notice.sped_up", amount_of(&runner.engine, sped.fee), true)
            }
            Err(error) => (wallet_key(&error), String::new(), false),
        }
    }
}

/// The addresses whose signatures a shared spend still needs, any `missing` of them.
fn waiting_for(spend: &SharedSpend) -> Vec<String> {
    let signed = spend.signed();
    let others = (0..spend.shared.keys.len()).filter(|at| !signed.contains(at));
    others.map(|at| spend.shared.signer(at)).collect()
}
