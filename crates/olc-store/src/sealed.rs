//! A wallet sealed with a password, as Bitcoin Core's encryptwallet seals its keys. Keys and words
//! go through Argon2id and ChaCha20-Poly1305 exactly as a sealed backup does, so the sealed part of
//! the file is a backup any build can restore. What a locked wallet still needs to find its coins
//! and receive (public keys, names, contacts, shared wallets) stays readable beside it, as Bitcoin
//! Core keeps its address book and public keys readable. Nothing here prints a key or a password.
use crate::wallet_file::{
    hex, keys_of, open_bytes, password_key, random, seal_bytes, unhex, wallet_of, write_private, Backup, Keys,
    DAMAGED_WALLET, OLD_KEY_FILE, WALLET_FILE,
};
use olc_wallet::shared::Shared;
use olc_wallet::{Account, Contact, Wallet};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io::ErrorKind;
use std::path::Path;
use zeroize::Zeroizing;

pub const WALLET_LOCKED: &str = "The wallet is locked.";
pub const WRONG_WALLET_PASSWORD: &str = "That password does not open this wallet.";
pub const ALREADY_SEALED: &str = "The wallet already has a password.";
pub const NOT_SEALED: &str = "The wallet has no password.";
pub const EMPTY_PASSWORD: &str = "The password is empty.";
const NEWER_FILE: &str = "A newer build sealed this wallet. Open it with that build.";
/// The first and only layout so far. A build that meets a higher number leaves the file alone.
const LAYOUT: u32 = 1;

/// What a locked wallet may change, kept readable.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Readable {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    shared: Vec<Shared>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    labels: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    contacts: Vec<Contact>,
    /// The words' account, an xpub and its next key, so the locked wallet still makes receiving
    /// addresses. No secret comes from it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    watch: Option<Account>,
}

impl Readable {
    fn of(wallet: &Wallet) -> Self {
        let (shared, labels, contacts) = (wallet.shared.clone(), wallet.labels.clone(), wallet.contacts.clone());
        Self { shared, labels, contacts, watch: wallet.watch_account() }
    }

    /// These are newer than the sealed copy, which only changes while the wallet is open. Keys
    /// the locked wallet made from its account get their secrets from the words here.
    pub(crate) fn put_in(self, wallet: &mut Wallet) {
        wallet.shared = self.shared;
        wallet.labels = self.labels;
        wallet.contacts = self.contacts;
        if let Some(watch) = self.watch {
            // A failure leaves those keys out, and the caller finds them missing.
            let _ = wallet.catch_up_words(watch.next);
        }
    }
}

/// `wallet.json` once it has a password. Older builds find no `keys` in it, call it damaged and
/// leave it as it is, rather than making a new wallet over it.
#[derive(Serialize, Deserialize)]
pub(crate) struct SealedFile {
    olc_wallet_sealed: u32,
    /// Each key's public half in hex, in order.
    publics: Vec<String>,
    receiving: usize,
    /// Whether the sealed part holds seed words, and whether the person wrote them down.
    #[serde(default)]
    words: bool,
    #[serde(default)]
    words_written: bool,
    #[serde(flatten)]
    readable: Readable,
    salt: String,
    nonce: String,
    /// Every key and the words as a backup holds them, sealed.
    sealed: String,
}

/// The key that seals an open wallet. It lives only while the wallet is open and is wiped when
/// dropped; the password itself is never kept.
pub struct Seal {
    salt: [u8; 16],
    key: Zeroizing<[u8; 32]>,
}

/// What a locked wallet's file says about its words, which stay sealed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SealedWords {
    pub has: bool,
    pub written: bool,
}

fn publics_of(wallet: &Wallet) -> Vec<String> {
    wallet.keys().map(|(_, public)| hex(&public)).collect()
}

/// The sealed file in `text`, `None` for a plain one.
pub(crate) fn parse(text: &str) -> Result<Option<SealedFile>, String> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
        return Ok(None);
    };
    let Some(layout) = value.get("olc_wallet_sealed") else {
        return Ok(None);
    };
    if layout.as_u64().is_none_or(|layout| layout > u64::from(LAYOUT)) {
        return Err(NEWER_FILE.to_string());
    }
    serde_json::from_value(value).map(Some).map_err(|_| DAMAGED_WALLET.to_string())
}

/// The sealed file in `directory`, `None` when the wallet there is plain or not made yet.
pub(crate) fn read(directory: &Path) -> Result<Option<SealedFile>, String> {
    match fs::read_to_string(directory.join(WALLET_FILE)) {
        Ok(text) => parse(&text),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(_) => Err(DAMAGED_WALLET.to_string()),
    }
}

/// Whether the wallet in `directory` has a password, and what it says of its words.
pub fn sealed_words(directory: &Path) -> Option<SealedWords> {
    let file = read(directory).ok()??;
    Some(SealedWords { has: file.words, written: file.words_written })
}

/// The wallet as it runs while locked: it finds its coins, receives and mines, and signs nothing.
pub(crate) fn locked_wallet(file: &SealedFile) -> Result<Wallet, String> {
    let publics: Option<Vec<[u8; 33]>> =
        file.publics.iter().map(|text| unhex(text).and_then(|bytes| bytes.try_into().ok())).collect();
    let publics = publics.ok_or(DAMAGED_WALLET)?;
    let mut wallet = Wallet::from_publics(&publics, file.receiving).map_err(|_| DAMAGED_WALLET)?;
    file.readable.clone().put_in(&mut wallet);
    wallet.watch = file.readable.watch.clone();
    Ok(wallet)
}

fn write(directory: &Path, file: &SealedFile) -> Result<(), String> {
    let text = serde_json::to_string_pretty(file).map_err(|error| error.to_string())?;
    write_private(&directory.join(WALLET_FILE), &text)
}

fn sealed_file(wallet: &Wallet, seal: &Seal) -> Result<SealedFile, String> {
    let plain = Zeroizing::new(serde_json::to_vec(&keys_of(wallet)?).map_err(|error| error.to_string())?);
    let (nonce, sealed) = seal_bytes(&seal.key, &plain)?;
    Ok(SealedFile {
        olc_wallet_sealed: LAYOUT,
        publics: publics_of(wallet),
        receiving: wallet.receiving(),
        words: wallet.seed.is_some(),
        words_written: wallet.seed.as_ref().is_some_and(|seed| seed.written),
        readable: Readable::of(wallet),
        salt: hex(&seal.salt),
        nonce: hex(&nonce),
        sealed: hex(&sealed),
    })
}

fn new_seal(password: &str) -> Result<Seal, String> {
    if password.is_empty() {
        return Err(EMPTY_PASSWORD.to_string());
    }
    let salt: [u8; 16] = random()?;
    Ok(Seal { salt, key: Zeroizing::new(password_key(password, &salt)?) })
}

/// Seals the plain wallet in `directory` with `password` (Bitcoin Core's encryptwallet). The
/// answer is the seal, for a node that keeps the wallet open a while longer.
pub fn seal_wallet(directory: &Path, wallet: &Wallet, password: &str) -> Result<Seal, String> {
    if read(directory)?.is_some() {
        return Err(ALREADY_SEALED.to_string());
    }
    let seal = new_seal(password)?;
    write(directory, &sealed_file(wallet, &seal)?)?;
    // An older node's single key file would keep a key readable beside the sealed ones.
    let old = directory.join(OLD_KEY_FILE);
    let old_key = fs::read_to_string(&old).ok().and_then(|text| crate::parse_secret(&text));
    let kept = old_key.is_some_and(|secret| (0..wallet.key_count()).any(|index| wallet.export_secret(index) == Some(secret)));
    if kept {
        fs::remove_file(&old).map_err(|error| error.to_string())?;
    }
    Ok(seal)
}

/// Opens the sealed wallet in `directory` (Bitcoin Core's walletpassphrase). A wrong password
/// fails here and changes nothing.
pub fn open_wallet(directory: &Path, password: &str) -> Result<(Wallet, Seal), String> {
    let file = read(directory)?.ok_or(NOT_SEALED)?;
    let salt: [u8; 16] = unhex(&file.salt).and_then(|bytes| bytes.try_into().ok()).ok_or(DAMAGED_WALLET)?;
    let nonce: [u8; 12] = unhex(&file.nonce).and_then(|bytes| bytes.try_into().ok()).ok_or(DAMAGED_WALLET)?;
    let sealed = unhex(&file.sealed).ok_or(DAMAGED_WALLET)?;
    let key = Zeroizing::new(password_key(password, &salt)?);
    let plain = Zeroizing::new(open_bytes(&key, &nonce, &sealed).ok_or(WRONG_WALLET_PASSWORD)?);
    let keys: Keys = serde_json::from_slice(&plain).map_err(|_| DAMAGED_WALLET)?;
    let mut wallet = wallet_of(&keys)?;
    file.readable.put_in(&mut wallet);
    if publics_of(&wallet) != file.publics {
        return Err(DAMAGED_WALLET.to_string());
    }
    wallet.set_receiving(file.receiving).map_err(|_| DAMAGED_WALLET)?;
    Ok((wallet, Seal { salt, key }))
}

/// Saves an open wallet, sealing its keys again under the same password.
pub fn save_open(directory: &Path, wallet: &Wallet, seal: &Seal) -> Result<(), String> {
    let file = read(directory)?.ok_or(NOT_SEALED)?;
    // The password changed elsewhere since this seal was made. Its keys are the ones to keep.
    if file.salt != hex(&seal.salt) {
        return Err(WALLET_LOCKED.to_string());
    }
    write(directory, &sealed_file(wallet, seal)?)
}

/// Saves what a locked wallet may change. Secrets and words change only while it is open, so a
/// wallet that holds a secret is refused, and so is one whose keys differ from the file's, but
/// for receiving keys it added from the words' account, whose secrets the words keep.
pub(crate) fn save_locked(directory: &Path, wallet: &Wallet, file: SealedFile) -> Result<(), String> {
    let publics = publics_of(wallet);
    let kept = publics.get(..file.publics.len()).is_some_and(|earlier| earlier == file.publics.as_slice());
    let made = publics.len() - file.publics.len().min(publics.len());
    let from_account = match (&wallet.watch, &file.readable.watch) {
        (Some(now), Some(before)) => now.xpub == before.xpub && u64::from(now.next.saturating_sub(before.next)) == made as u64,
        _ => made == 0,
    };
    if wallet.holds_secrets() || !kept || !from_account {
        return Err(WALLET_LOCKED.to_string());
    }
    let readable = Readable::of(wallet);
    write(directory, &SealedFile { publics, receiving: wallet.receiving(), readable, ..file })
}

/// Seals the wallet again under `new` once `old` opens it. A wrong `old` changes nothing.
pub fn change_password(directory: &Path, old: &str, new: &str) -> Result<Seal, String> {
    if new.is_empty() {
        return Err(EMPTY_PASSWORD.to_string());
    }
    let (wallet, _) = open_wallet(directory, old)?;
    let seal = new_seal(new)?;
    write(directory, &sealed_file(&wallet, &seal)?)?;
    Ok(seal)
}

/// A backup of a locked wallet: its keys stay sealed with the wallet's password, in the layout of
/// a backup with a password, so restoring asks for that password in this build and older ones.
pub fn back_up_locked(directory: &Path, path: &Path) -> Result<(), String> {
    let file = read(directory)?.ok_or(NOT_SEALED)?;
    let backup = Backup {
        olc_wallet_backup: 1,
        keys: None,
        salt: Some(file.salt),
        nonce: Some(file.nonce),
        sealed: Some(file.sealed),
        newer: Some(file.readable),
    };
    let text = serde_json::to_string_pretty(&backup).map_err(|error| error.to_string())?;
    write_private(path, &text)
}

#[cfg(test)]
#[path = "sealed_tests.rs"]
mod tests;
