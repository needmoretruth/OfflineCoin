//! A wallet several people share: coins under a gamma lock that any `need` of its keys open, as
//! Bitcoin's m-of-n multisig, up to 20 keys. Each person keeps their own key, and a spend goes
//! from one to the next as text, as a lock spend does, gathering signatures until it has enough.
use crate::{format_address, Wallet};
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use olc_core::types::version_for;
use olc_core::{Address, OutPoint, Tx, TxIn, TxOut, SEQUENCE_FINAL};
use olc_crypto::{address_of, sign, verify, Hash, PublicKey};
use olc_lang::gamma::MAX_KEYS;
use olc_lang::words::{assemble_in, lock_from_text_in, words_of_in, Dialect, Word, MOST_SIGNATURE_CHECKS};
use olc_lang::{ALPHA, GAMMA};
use serde::{Deserialize, Serialize};

/// What a shared wallet's text starts with. A person pays it as they would an address.
pub const SHARED_START: &str = "olcshared1";
/// What a spend of one starts with on its way between signers.
pub const SHARED_SPEND_START: &str = "olcsharedspend1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SharedError {
    /// One to 20 keys, and no more signatures needed than keys.
    Shape,
    /// A key is not a public key, or comes twice.
    BadKey,
    NotEnoughCoins,
    BadText,
    /// It still needs this many signatures.
    Waiting(usize),
}

/// Who opens a shared wallet: `need` of `keys`, in this order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Shared {
    pub need: u8,
    pub keys: Vec<Vec<u8>>,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.is_ascii() || !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len()).step_by(2).map(|at| u8::from_str_radix(text.get(at..at + 2)?, 16).ok()).collect()
}

impl Shared {
    pub fn new(need: usize, keys: Vec<Vec<u8>>) -> Result<Self, SharedError> {
        let most = usize::try_from(MAX_KEYS).unwrap_or(20);
        if need == 0 || need > keys.len() || keys.len() > most {
            return Err(SharedError::Shape);
        }
        for (at, key) in keys.iter().enumerate() {
            let valid = key.len() == 33 && k256::PublicKey::from_sec1_bytes(key).is_ok();
            if !valid || keys[..at].contains(key) {
                return Err(SharedError::BadKey);
            }
        }
        let shared = Self { need: need as u8, keys };
        // A lock that did not build would be empty, and an empty lock opens for anyone.
        match lock_from_text_in(Dialect::Gamma, &shared.words()) {
            Ok((lock, _)) if !lock.is_empty() => Ok(shared),
            _ => Err(SharedError::Shape),
        }
    }

    /// The lock's words: `need`, the keys, their count and CHECKMULTISIG.
    pub fn words(&self) -> String {
        let mut words = format!("{}", self.need);
        for key in &self.keys {
            words.push_str(" 0x");
            words.push_str(&hex(key));
        }
        words.push_str(&format!(" {} CHECKMULTISIG", self.keys.len()));
        words
    }

    pub fn lock(&self) -> Vec<u8> {
        lock_from_text_in(Dialect::Gamma, &self.words()).map(|(lock, _)| lock).unwrap_or_default()
    }

    /// Reads a shared wallet back from a coin's lock.
    pub fn from_lock(lock: &[u8]) -> Option<Self> {
        let words = words_of_in(Dialect::Gamma, lock);
        let words: Vec<&str> = words.split_whitespace().collect();
        let [need, keys @ .., count, "CHECKMULTISIG"] = words.as_slice() else {
            return None;
        };
        let keys: Option<Vec<Vec<u8>>> = keys.iter().map(|key| unhex(key.strip_prefix("0x")?)).collect();
        let keys = keys?;
        if count.parse::<usize>().ok()? != keys.len() {
            return None;
        }
        let shared = Self::new(need.parse().ok()?, keys).ok()?;
        (shared.lock() == lock).then_some(shared)
    }

    /// The text a person hands out to be paid: the keys and how many must sign.
    pub fn to_text(&self) -> String {
        let keys: Vec<String> = self.keys.iter().map(|key| hex(key)).collect();
        format!("{SHARED_START}.{}.{}", self.need, keys.join("."))
    }

    pub fn from_text(text: &str) -> Result<Self, SharedError> {
        let mut parts = text.trim().split('.');
        if parts.next() != Some(SHARED_START) {
            return Err(SharedError::BadText);
        }
        let need = parts.next().and_then(|need| need.parse().ok()).ok_or(SharedError::BadText)?;
        let keys: Option<Vec<Vec<u8>>> = parts.map(unhex).collect();
        Self::new(need, keys.ok_or(SharedError::BadText)?)
    }

    /// The signature checks one input costs, as Bitcoin counts a multisig: one for each key.
    fn checks(&self) -> usize {
        self.keys.len()
    }

    /// The keys of this wallet that are in it, by their place.
    pub fn held_by(&self, wallet: &Wallet) -> Vec<usize> {
        let held = |at: &usize| wallet.keys.iter().any(|key| key.public.as_slice() == self.keys[*at].as_slice());
        (0..self.keys.len()).filter(held).collect()
    }

    /// The address of the key at `at`, as a person reads it.
    pub fn signer(&self, at: usize) -> String {
        let key = self.keys.get(at).and_then(|key| <PublicKey>::try_from(key.as_slice()).ok());
        key.map(|key| format_address(&address_of(&key))).unwrap_or_default()
    }
}

/// A spend of a shared wallet's coins, with the signatures gathered so far.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharedSpend {
    pub tx: Tx,
    pub shared: Shared,
    pub coin: Option<Hash>,
    /// For each input, a signature for each key, or none yet.
    signatures: Vec<Vec<Option<Vec<u8>>>>,
}

impl SharedSpend {
    /// Pays `amount` to `to` from `coins`, the change going back to the shared wallet. Takes no
    /// more coins than one transaction's signature checks allow.
    pub fn new(
        shared: &Shared,
        coins: &[(OutPoint, u64)],
        to: Address,
        amount: u64,
        fee: u64,
        coin: Option<Hash>,
    ) -> Result<Self, SharedError> {
        let needed = amount.checked_add(fee).ok_or(SharedError::NotEnoughCoins)?;
        let most_inputs = (MOST_SIGNATURE_CHECKS / shared.checks()).max(1);
        let mut chosen = Vec::new();
        let mut gathered = 0u64;
        for (point, value) in coins.iter().take(most_inputs) {
            if gathered >= needed && !chosen.is_empty() {
                break;
            }
            gathered = gathered.saturating_add(*value);
            chosen.push(*point);
        }
        if amount == 0 || gathered < needed {
            return Err(SharedError::NotEnoughCoins);
        }
        let mut outputs = vec![TxOut { value: amount, language: ALPHA, lock: to.to_vec() }];
        let change = gathered - needed;
        if change > 0 {
            outputs.push(TxOut { value: change, language: GAMMA, lock: shared.lock() });
        }
        let input = |previous: &OutPoint| TxIn { previous: *previous, unlock: Vec::new(), sequence: SEQUENCE_FINAL };
        let tx = Tx {
            version: version_for(coin.as_ref()),
            inputs: chosen.iter().map(input).collect(),
            outputs,
            coinbase_height: None,
            lock_time: 0,
        };
        let signatures = vec![vec![None; shared.keys.len()]; chosen.len()];
        Ok(Self { tx, shared: shared.clone(), coin, signatures })
    }

    fn digest(&self, input: usize) -> Hash {
        self.tx.signature_hash(input, &self.shared.lock(), self.coin.as_ref())
    }

    /// Adds every signature this wallet's keys can make. Says how many keys signed.
    pub fn fill(&mut self, wallet: &Wallet) -> usize {
        let held = self.shared.held_by(wallet);
        for input in 0..self.signatures.len() {
            let digest = self.digest(input);
            for at in &held {
                let key = wallet.keys.iter().find(|key| key.public.as_slice() == self.shared.keys[*at].as_slice());
                let signature = key.and_then(|key| sign(&key.secret, &digest));
                self.signatures[input][*at] = signature.map(|signature| signature.to_vec());
            }
        }
        held.len()
    }

    /// The places of the keys that signed every input.
    pub fn signed(&self) -> Vec<usize> {
        let all = |at: &usize| self.signatures.iter().all(|row| row.get(*at).is_some_and(Option::is_some));
        (0..self.shared.keys.len()).filter(all).collect()
    }

    /// How many more signatures it waits for.
    pub fn missing(&self) -> usize {
        usize::from(self.shared.need).saturating_sub(self.signed().len())
    }

    /// The finished transaction, with the first `need` signatures in key order on each input.
    pub fn finish(mut self) -> Result<Tx, SharedError> {
        let missing = self.missing();
        if missing > 0 {
            return Err(SharedError::Waiting(missing));
        }
        let signed = self.signed();
        let need = usize::from(self.shared.need);
        for (input, row) in self.signatures.iter().enumerate() {
            let words: Vec<Word> = signed.iter().take(need).filter_map(|at| row[*at].clone()).map(Word::Data).collect();
            let (unlock, _) = assemble_in(Dialect::Gamma, &words).map_err(|_| SharedError::BadText)?;
            self.tx.inputs[input].unlock = unlock;
        }
        Ok(self.tx)
    }

    pub fn to_text(&self) -> String {
        let bytes = postcard::to_allocvec(self).unwrap_or_default();
        format!("{SHARED_SPEND_START}.{}", hex(&bytes))
    }

    /// Reads a spend another signer passed on. A signature that does not check is dropped here,
    /// so a wrong one can never stand in for a missing one.
    pub fn from_text(text: &str) -> Result<Self, SharedError> {
        let (start, bytes) = text.trim().split_once('.').ok_or(SharedError::BadText)?;
        if start != SHARED_SPEND_START {
            return Err(SharedError::BadText);
        }
        let bytes = unhex(bytes).ok_or(SharedError::BadText)?;
        let mut spend: Self = postcard::from_bytes(&bytes).map_err(|_| SharedError::BadText)?;
        let shape = Shared::new(usize::from(spend.shared.need), spend.shared.keys.clone());
        let rows = spend.signatures.len() == spend.tx.inputs.len()
            && spend.signatures.iter().all(|row| row.len() == spend.shared.keys.len());
        if shape.is_err() || !rows {
            return Err(SharedError::BadText);
        }
        for input in 0..spend.signatures.len() {
            let digest = spend.digest(input);
            for at in 0..spend.shared.keys.len() {
                let key = spend.shared.keys[at].clone();
                let slot = &mut spend.signatures[input][at];
                if slot.as_ref().is_some_and(|signature| !verify(&key, &digest, signature)) {
                    *slot = None;
                }
            }
        }
        Ok(spend)
    }
}

#[cfg(test)]
#[path = "shared_tests.rs"]
mod tests;
