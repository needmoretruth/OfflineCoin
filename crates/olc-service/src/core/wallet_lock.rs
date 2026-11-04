//! A wallet's password, as Bitcoin Core's encryptwallet, walletpassphrase, walletlock and
//! walletpassphrasechange: sealing the keys, opening the wallet for a while, locking it again and
//! changing the password. While locked a node still counts its coins, receives and mines; only
//! what needs a secret key waits for the password. The password itself is never kept: an open
//! wallet holds the key made from it, until the time a person picked runs out.
use super::folder::Folder;
use super::host::{Host, State};
use super::ops::Book;
use olc_node::Runner;
use olc_store::{
    change_password, node_runs_on, open_wallet, save_open, save_wallet, seal_wallet, sealed_words, Seal,
    SealedWords, ALREADY_SEALED, EMPTY_PASSWORD, NODE_ON_FOLDER, NOT_SEALED, WALLET_LOCKED, WRONG_WALLET_PASSWORD,
};
use olc_wallet::Wallet;
use std::path::Path;
use std::time::{Duration, Instant};
use zeroize::Zeroize;

/// How long a wallet stays open when a person does not say: Bitcoin Core's GUI asks for about as
/// long as one payment takes.
pub const OPEN_FOR: u64 = 5 * 60;
/// The longest one opening lasts. Longer belongs to a wallet without a password.
pub const OPEN_AT_MOST: u64 = 24 * 60 * 60;
/// Nothing stays open on a folder, since nothing keeps running there.
pub(super) const UNLOCK_NEEDS_NODE: &str = "notice.unlock_needs_node";

/// A password on its way from a field to the wallet. It never shows in a log and is wiped once used.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Password(pub String);

impl std::fmt::Debug for Password {
    fn fmt(&self, out: &mut std::fmt::Formatter) -> std::fmt::Result {
        out.write_str("Password(..)")
    }
}

impl Drop for Password {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

/// Something to do with the wallet's password.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PasswordOrder {
    /// Seals the keys with a password. The wallet locks at once, as Bitcoin Core's does.
    Encrypt(Password),
    /// Opens a sealed wallet for this many seconds.
    Unlock { password: Password, seconds: u64 },
    /// Locks it again now.
    Lock,
    /// Seals it under a new password once the old one opens it.
    Change { old: Password, new: Password },
    /// How the wallet stands, in words, for the command line.
    State,
}

/// The password part of the wallet as the screen shows it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LockView {
    /// The keys are sealed with a password.
    pub sealed: bool,
    /// Seconds until it locks again, while it is open.
    pub open_for: Option<u64>,
    /// Wrong passwords typed so far, so a sheet waiting on one can tell.
    pub wrong: u64,
}

/// What a running node keeps about the password.
#[derive(Default)]
pub struct LockState {
    /// What the file says of the sealed words; `None` while the wallet has no password.
    sealed: Option<SealedWords>,
    /// The seal and when the wallet locks again, while it is open.
    open: Option<(Seal, Instant)>,
    wrong: u64,
}

impl LockState {
    pub fn read(directory: &Path) -> Self {
        Self { sealed: sealed_words(directory), ..Self::default() }
    }
}

/// A failure from the wallet file as the key of its notice.
fn fault(error: String) -> String {
    let key = match error.as_str() {
        WALLET_LOCKED => "notice.wallet_locked",
        WRONG_WALLET_PASSWORD => "notice.wrong_wallet_password",
        ALREADY_SEALED => "notice.already_sealed",
        NOT_SEALED => "notice.not_sealed",
        EMPTY_PASSWORD => "notice.empty_password",
        NODE_ON_FOLDER => "notice.node_on_wallet",
        _ => return error,
    };
    key.to_string()
}

fn seconds_text(seconds: u64) -> String {
    seconds.to_string()
}

/// Saves the wallet of a running node: sealed again with its seal while open, else as it is.
pub(super) fn save(state: &State, directory: &Path, wallet: &Wallet) -> Result<(), String> {
    let lock = state.purse.lock().map_err(|_| fault(WALLET_LOCKED.to_string()))?;
    let saved = match &lock.open {
        Some((seal, _)) => save_open(directory, wallet, seal),
        None => save_wallet(directory, wallet),
    };
    saved.map_err(fault)
}

/// Locks the wallet once its time is up, or forgets the seal when the wallet came back locked from
/// its file, as it does when the node moves to another coin. Runs a few times a second.
pub(super) fn tick(state: &State, runner: &mut Runner, directory: &Path) {
    let Ok(mut lock) = state.purse.lock() else {
        return;
    };
    let Some((_, until)) = &lock.open else {
        return;
    };
    let over = Instant::now() >= *until;
    if over {
        let wallet = runner.engine.wallet_mut();
        *wallet = wallet.locked();
    }
    if over || !runner.engine.wallet().is_open() {
        lock.open = None;
        lock.sealed = sealed_words(directory);
    }
}

/// The password part of the view, and whether the sealed wallet has words.
pub(super) fn view(state: &State) -> (LockView, Option<SealedWords>) {
    let Ok(lock) = state.purse.lock() else {
        return (LockView::default(), None);
    };
    let open_for = lock.open.as_ref().map(|(_, until)| until.saturating_duration_since(Instant::now()).as_secs().max(1));
    (LockView { sealed: lock.sealed.is_some(), open_for, wrong: lock.wrong }, lock.sealed)
}

impl Host {
    fn note_wrong(&self, error: &str) {
        if error == WRONG_WALLET_PASSWORD {
            if let Ok(mut lock) = self.state.purse.lock() {
                lock.wrong += 1;
            }
        }
    }

    fn encrypt(&self, password: &Password) -> Result<(&'static str, String), String> {
        let directory = self.wallet_folder();
        // The hash of a password takes a while, and mining should not wait for it.
        let wallet = self.wallet_view(Wallet::clone)?;
        let seal = seal_wallet(&directory, &wallet, &password.0)?;
        self.on_runner(|runner| {
            // A name given while the hash ran is in the engine's copy: seal that one.
            let wallet = runner.engine.wallet_mut();
            save_open(&directory, wallet, &seal)?;
            *wallet = wallet.locked();
            if let Ok(mut lock) = self.state.purse.lock() {
                *lock = LockState::read(&directory);
            }
            Ok(("notice.wallet_sealed", String::new()))
        })?
    }

    fn unlock(&self, password: &Password, seconds: u64) -> Result<(&'static str, String), String> {
        let directory = self.wallet_folder();
        let seconds = seconds.clamp(1, OPEN_AT_MOST);
        let opened = open_wallet(&directory, &password.0);
        let (mut opened, seal) = opened.inspect_err(|error| self.note_wrong(error))?;
        self.on_runner(|runner| {
            let wallet = runner.engine.wallet_mut();
            if wallet.keys().ne(opened.keys()) {
                return Err(WALLET_LOCKED.to_string());
            }
            // Names and contacts saved while the hash ran are newer than the file it read.
            opened.labels = wallet.labels.clone();
            opened.contacts = wallet.contacts.clone();
            opened.shared = wallet.shared.clone();
            let _ = opened.set_receiving(wallet.receiving());
            *wallet = opened;
            if let Ok(mut lock) = self.state.purse.lock() {
                lock.open = Some((seal, Instant::now() + Duration::from_secs(seconds)));
            }
            Ok(("notice.wallet_opened", seconds_text(seconds)))
        })?
    }

    fn lock_now(&self) -> Result<(&'static str, String), String> {
        let directory = self.wallet_folder();
        self.on_runner(|runner| {
            let Ok(mut lock) = self.state.purse.lock() else {
                return Err(WALLET_LOCKED.to_string());
            };
            if lock.sealed.is_none() {
                return Err(NOT_SEALED.to_string());
            }
            let wallet = runner.engine.wallet_mut();
            *wallet = wallet.locked();
            lock.open = None;
            lock.sealed = sealed_words(&directory);
            Ok(("notice.wallet_locked_now", String::new()))
        })?
    }

    fn change(&self, old: &Password, new: &Password) -> Result<(&'static str, String), String> {
        let directory = self.wallet_folder();
        let seal = change_password(&directory, &old.0, &new.0).inspect_err(|error| self.note_wrong(error))?;
        self.on_runner(|runner| {
            let Ok(mut lock) = self.state.purse.lock() else {
                return Err(WALLET_LOCKED.to_string());
            };
            let wallet = runner.engine.wallet();
            // What changed while the hashes ran is saved again under the new seal.
            match lock.open.take() {
                Some((_, until)) => {
                    save_open(&directory, wallet, &seal)?;
                    lock.open = Some((seal, until));
                }
                None => save_wallet(&directory, wallet)?,
            }
            Ok(("notice.password_changed", String::new()))
        })?
    }

    fn lock_state(&self) -> String {
        let (view, _) = view(&self.state);
        match (view.sealed, view.open_for) {
            (false, _) => "This wallet has no password.".to_string(),
            (true, None) => "Locked. Its keys are sealed with a password.".to_string(),
            (true, Some(seconds)) => format!("Open for {seconds} more seconds, then locked."),
        }
    }

    pub(super) fn carry_password(&self, order: &PasswordOrder) -> Result<(&'static str, String), String> {
        let done = match order {
            PasswordOrder::Encrypt(password) => self.encrypt(password),
            PasswordOrder::Unlock { password, seconds } => self.unlock(password, *seconds),
            PasswordOrder::Lock => self.lock_now(),
            PasswordOrder::Change { old, new } => self.change(old, new),
            PasswordOrder::State => Ok(("notice.lock_state", self.lock_state())),
        };
        done.map_err(fault)
    }
}

impl Folder {
    /// The same orders on a wallet's folder while no node runs on it. Nothing stays open here,
    /// since nothing keeps running: a word that needs the keys asks a running node.
    pub(super) fn carry_password(&self, order: &PasswordOrder) -> Result<(&'static str, String), String> {
        let directory = &self.directory;
        // A node on this wallet holds it, open or not: its window or OLC Core changes it there.
        if !matches!(order, PasswordOrder::State) && node_runs_on(directory) {
            return Err(fault(NODE_ON_FOLDER.to_string()));
        }
        let done = match order {
            PasswordOrder::Encrypt(password) => {
                let wallet = self.wallet_view(Wallet::clone)?;
                seal_wallet(directory, &wallet, &password.0).map(|_| ("notice.wallet_sealed", String::new()))
            }
            PasswordOrder::Change { old, new } => {
                change_password(directory, &old.0, &new.0).map(|_| ("notice.password_changed", String::new()))
            }
            PasswordOrder::Unlock { .. } => Err(UNLOCK_NEEDS_NODE.to_string()),
            PasswordOrder::Lock | PasswordOrder::State => match sealed_words(directory) {
                Some(_) => Ok(("notice.lock_state", "Locked. Its keys are sealed with a password.".to_string())),
                None => Ok(("notice.lock_state", "This wallet has no password.".to_string())),
            },
        };
        done.map_err(fault)
    }
}

#[cfg(test)]
#[path = "wallet_lock_tests.rs"]
mod tests;
