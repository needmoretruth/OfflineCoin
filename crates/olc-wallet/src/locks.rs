//! Coins under a beta lock. A person writes what opens the lock in words. Each wallet fills in the
//! signatures it can make, and a lock that needs two people passes the spend along as text.
use crate::{Wallet, WalletError};
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use olc_core::{Address, OutPoint, Tx, TxIn, TxOut};
use olc_crypto::{sign, Hash};
use olc_lang::words::{assemble, parse_words, text_of_words, Word, WordError};
use olc_lang::ALPHA;

const TEXT_START: &str = "olcspend1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LockError {
    Words(WordError),
    /// The coin is worth no more than its fee.
    TooSmall,
    BadText,
    Wallet(WalletError),
}

impl From<WordError> for LockError {
    fn from(error: WordError) -> Self {
        Self::Words(error)
    }
}

impl From<WalletError> for LockError {
    fn from(error: WalletError) -> Self {
        Self::Wallet(error)
    }
}

/// A spend of one locked coin, whole or waiting for a signature.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LockSpend {
    pub tx: Tx,
    pub lock: Vec<u8>,
    pub unlock: Vec<Word>,
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn from_hex(text: &str) -> Option<Vec<u8>> {
    let digit = |byte: u8| char::from(byte).to_digit(16).and_then(|value| u8::try_from(value).ok());
    let pair = |pair: &[u8]| Some(digit(*pair.first()?)? * 16 + digit(*pair.get(1)?)?);
    text.as_bytes().chunks(2).map(pair).collect()
}

impl LockSpend {
    /// Sends the coin at `previous`, less `fee`, to `to`.
    pub fn new(
        previous: OutPoint,
        coin: &TxOut,
        to: Address,
        fee: u64,
        unlock: &str,
    ) -> Result<Self, LockError> {
        let value = coin.value.checked_sub(fee).filter(|value| *value > 0);
        let value = value.ok_or(LockError::TooSmall)?;
        let output = TxOut { value, language: ALPHA, lock: to.to_vec() };
        let tx = Tx {
            version: 1,
            inputs: vec![TxIn { previous, unlock: Vec::new() }],
            outputs: vec![output],
            coinbase_height: None,
        };
        Ok(Self { tx, lock: coin.lock.clone(), unlock: parse_words(unlock)? })
    }

    fn digest(&self) -> Hash {
        self.tx.signature_hash(0, &self.lock)
    }

    /// Puts in every signature and key this wallet holds.
    pub fn fill(&mut self, wallet: &Wallet) {
        let digest = self.digest();
        for word in &mut self.unlock {
            let filled = match word {
                Word::SignBy(address) => wallet.signature(address, &digest),
                Word::KeyOf(address) => wallet.public_of(address),
                _ => None,
            };
            if let Some(filled) = filled {
                *word = Word::Data(filled);
            }
        }
    }

    /// Whose signature it still waits for.
    pub fn waiting_for(&self) -> Vec<String> {
        let waiting = self.unlock.iter().filter_map(|word| match word {
            Word::SignBy(address) | Word::KeyOf(address) => Some(address.clone()),
            _ => None,
        });
        waiting.collect()
    }

    /// The finished transaction, once nothing is missing.
    pub fn finish(mut self) -> Result<Tx, LockError> {
        let (unlock, _) = assemble(&self.unlock)?;
        let input = self.tx.inputs.first_mut().ok_or(LockError::BadText)?;
        input.unlock = unlock;
        Ok(self.tx)
    }

    /// Text to hand to the other signer.
    pub fn to_text(&self) -> String {
        let bytes = postcard::to_allocvec(&(&self.tx, &self.lock)).unwrap_or_default();
        format!("{TEXT_START}.{}.{}", to_hex(&bytes), text_of_words(&self.unlock))
    }

    pub fn from_text(text: &str) -> Result<Self, LockError> {
        let mut parts = text.trim().splitn(3, '.');
        let (start, bytes, words) = (parts.next(), parts.next(), parts.next());
        let (Some(TEXT_START), Some(bytes), Some(words)) = (start, bytes, words) else {
            return Err(LockError::BadText);
        };
        let bytes = from_hex(bytes).ok_or(LockError::BadText)?;
        let (tx, lock): (Tx, Vec<u8>) = postcard::from_bytes(&bytes).map_err(|_| LockError::BadText)?;
        Ok(Self { tx, lock, unlock: parse_words(words)? })
    }
}

impl Wallet {
    fn key_of(&self, address: &str) -> Option<&crate::Key> {
        let address = crate::parse_address(address)?;
        self.keys.iter().find(|key| key.address == address)
    }

    fn signature(&self, address: &str, digest: &Hash) -> Option<Vec<u8>> {
        let key = self.key_of(address)?;
        Some(sign(&key.secret, digest)?.to_vec())
    }

    fn public_of(&self, address: &str) -> Option<Vec<u8>> {
        Some(self.key_of(address)?.public.to_vec())
    }

    /// The address and public key of each key, for a lock that names keys.
    pub fn public_keys(&self) -> Vec<(String, Vec<u8>)> {
        let keys = self.keys.iter();
        keys.map(|key| (crate::format_address(&key.address), key.public.to_vec())).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use olc_core::SpendContext;
    use olc_core::LockLanguage;
    use olc_lang::beta::Beta;
    use olc_lang::words::{hash_lock, lock_from_text, time_lock, two_of_two};
    use olc_lang::BETA;
    use olc_crypto::hash;

    fn wallet(seed: u8) -> Wallet {
        Wallet::from_secret([seed; 32]).unwrap()
    }

    fn coin(text: &str) -> TxOut {
        TxOut { value: 1_000, language: BETA, lock: lock_from_text(text).unwrap().0 }
    }

    fn previous() -> OutPoint {
        OutPoint { txid: [3; 32], index: 0 }
    }

    fn opens(spend: LockSpend, height: u64) -> bool {
        let lock = spend.lock.clone();
        let digest = spend.tx.signature_hash(0, &lock);
        let tx = spend.finish().unwrap();
        Beta.check(&lock, &tx.inputs[0].unlock, &SpendContext { signature_hash: digest, height })
    }

    fn address(wallet: &Wallet) -> String {
        crate::format_address(&wallet.address())
    }

    #[test]
    fn a_secret_opens_a_hash_lock_with_no_wallet() {
        let lock = coin(&hash_lock(&hash(b"secret")[..20]));
        let spend = LockSpend::new(previous(), &lock, [1; 20], 10, "\"secret\"").unwrap();
        assert!(spend.waiting_for().is_empty());
        assert_eq!(spend.tx.outputs[0].value, 990);
        assert!(opens(spend, 0));
    }

    #[test]
    fn two_wallets_pass_a_spend_between_them_as_text() {
        let (first, second) = (wallet(1), wallet(2));
        let keys = (first.public_keys().remove(0).1, second.public_keys().remove(0).1);
        let lock = coin(&two_of_two(&keys.0, &keys.1));
        let unlock = format!("sig:{} sig:{}", address(&first), address(&second));

        let mut spend = LockSpend::new(previous(), &lock, [1; 20], 10, &unlock).unwrap();
        spend.fill(&first);
        assert_eq!(spend.waiting_for(), vec![address(&second)]);
        assert!(matches!(spend.clone().finish(), Err(LockError::Words(_))));

        let mut passed = LockSpend::from_text(&spend.to_text()).unwrap();
        assert_eq!(passed, spend);
        passed.fill(&second);
        assert!(passed.waiting_for().is_empty());
        assert!(opens(passed, 0));
    }

    #[test]
    fn a_wallet_opens_a_time_lock_with_its_key_at_the_height() {
        let owner = wallet(4);
        let lock = coin(&time_lock(50, &owner.address()));
        let unlock = format!("sig:{0} key:{0}", address(&owner));
        let mut spend = LockSpend::new(previous(), &lock, [1; 20], 10, &unlock).unwrap();
        spend.fill(&owner);
        assert!(!opens(spend.clone(), 49));
        assert!(opens(spend, 50));
    }

    #[test]
    fn a_coin_worth_its_fee_cannot_be_spent() {
        let lock = coin("1");
        assert_eq!(LockSpend::new(previous(), &lock, [1; 20], 1_000, "1"), Err(LockError::TooSmall));
    }

    #[test]
    fn text_that_is_no_spend_says_so() {
        assert_eq!(LockSpend::from_text("hello"), Err(LockError::BadText));
        assert_eq!(LockSpend::from_text("olcspend1.zz.1"), Err(LockError::BadText));
    }
}
