//! The wallet's newer parts on a running node: seed words to write down, shared wallets with what
//! they hold, spends of them that pass between signers, and speeding up a waiting payment.
use super::explorer::parse_hash;
use super::host::Host;
use super::host_locks::wallet_key;
use olc_core::amount::{format_coins, parse_coins};
use olc_node::shared::{SharedDone, SharedFailure};
use olc_node::Engine;
use olc_wallet::shared::{SharedError, SharedSpend};
use olc_wallet::parse_address;

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

/// The wallet beyond its keys.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WalletView {
    /// The seed words while the person has not yet said they wrote them down.
    pub words_to_write: Option<Words>,
    pub has_words: bool,
    pub shared: Vec<SharedView>,
}

pub(super) fn wallet_view(engine: &Engine) -> WalletView {
    let wallet = engine.wallet();
    let unwritten = wallet.seed.as_ref().filter(|seed| !seed.written);
    let shared = wallet.shared.iter().map(|shared| SharedView {
        text: shared.to_text(),
        need: usize::from(shared.need),
        signers: (0..shared.keys.len()).map(|at| shared.signer(at)).collect(),
        mine: shared.held_by(wallet),
        balance: engine.shared_coins(shared).iter().map(|(_, value)| value).sum(),
    });
    WalletView {
        words_to_write: unwritten.and_then(|seed| seed.mnemonic()).map(|words| Words(words.words())),
        has_words: wallet.seed.is_some(),
        shared: shared.collect(),
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
                ("notice.sped_up", format_coins(sped.fee), true)
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
