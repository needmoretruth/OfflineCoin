//! Seed words: BIP 39 words, in the English or the Korean list, and BIP 32 keys from them on a
//! BIP 44 path. The same words give the same keys on any computer.
//!
//! The path is m/44'/5196867'/account'/0/index. 5196867 is 0x4F4C43, the letters "OLC", picked
//! because it reads as the coin and sits far from the numbers SLIP 44 hands out.
use alloc::string::{String, ToString};
use crate::{SeedRecord, Wallet};
use alloc::vec::Vec;
use bip32::{ChildNumber, XPrv};
use olc_crypto::sha256;
use unicode_normalization::UnicodeNormalization;

/// The coin type of OLC's path, "OLC" in ASCII.
pub const COIN_TYPE: u32 = 0x004F_4C43;
/// Words a new wallet gets: 12, from 128 bits of randomness.
pub const NEW_WORDS: usize = 12;
const ROUNDS: u32 = 2048;

const ENGLISH: &str = include_str!("wordlists/english.txt");
/// Kept as the BIP's file has it, in decomposed letters (NFKD).
const KOREAN: &str = include_str!("wordlists/korean.txt");

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum WordList {
    English,
    Korean,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SeedError {
    /// Seed words come in 12, 15, 18, 21 or 24.
    Count(usize),
    /// The word at this place, counted from 1, is on neither list.
    Unknown(usize),
    /// Every word is on a list, but they do not add up: one is written wrong.
    Checksum,
    BadKey,
}

impl WordList {
    fn words(self) -> impl Iterator<Item = &'static str> {
        match self {
            Self::English => ENGLISH.lines(),
            Self::Korean => KOREAN.lines(),
        }
    }

    fn index_of(self, word: &str) -> Option<usize> {
        self.words().position(|known| known == word)
    }

    fn word(self, index: usize) -> Option<&'static str> {
        self.words().nth(index)
    }
}

/// Words and the randomness they stand for.
#[derive(Clone, PartialEq, Eq)]
pub struct Mnemonic {
    entropy: Vec<u8>,
    list: WordList,
}

impl core::fmt::Debug for Mnemonic {
    /// Words are a wallet's keys, so they stay out of every log.
    fn fmt(&self, out: &mut core::fmt::Formatter) -> core::fmt::Result {
        out.write_str("Mnemonic(..)")
    }
}

fn bit(bytes: &[u8], at: usize) -> bool {
    bytes[at / 8] & (0x80 >> (at % 8)) != 0
}

impl Mnemonic {
    /// Words for `entropy` of 16 to 32 bytes, a multiple of 4.
    pub fn from_entropy(entropy: &[u8], list: WordList) -> Result<Self, SeedError> {
        let bits = entropy.len() * 8;
        if !(128..=256).contains(&bits) || !bits.is_multiple_of(32) {
            return Err(SeedError::Count(0));
        }
        Ok(Self { entropy: entropy.to_vec(), list })
    }

    /// Reads words typed by a person, in either list, with any spacing or composed letters.
    pub fn parse(text: &str) -> Result<Self, SeedError> {
        let typed: Vec<String> = text.split_whitespace().map(|word| word.nfkd().collect::<String>().to_lowercase()).collect();
        if !matches!(typed.len(), 12 | 15 | 18 | 21 | 24) {
            return Err(SeedError::Count(typed.len()));
        }
        let list = match typed.first().map(String::as_str) {
            Some(first) if WordList::Korean.index_of(first).is_some() => WordList::Korean,
            _ => WordList::English,
        };
        let mut indices = Vec::with_capacity(typed.len());
        for (place, word) in typed.iter().enumerate() {
            indices.push(list.index_of(word).ok_or(SeedError::Unknown(place + 1))?);
        }
        let total = indices.len() * 11;
        let entropy_bits = total * 32 / 33;
        let mut bits = Vec::with_capacity(total);
        for index in &indices {
            bits.extend((0..11).rev().map(|shift| (index >> shift) & 1 == 1));
        }
        let mut entropy = alloc::vec![0u8; entropy_bits / 8];
        for (at, on) in bits.iter().take(entropy_bits).enumerate() {
            if *on {
                entropy[at / 8] |= 0x80 >> (at % 8);
            }
        }
        let check = sha256(&entropy);
        let fits = bits[entropy_bits..].iter().enumerate().all(|(at, on)| bit(&check, at) == *on);
        if !fits {
            return Err(SeedError::Checksum);
        }
        Ok(Self { entropy, list })
    }

    pub fn entropy(&self) -> &[u8] {
        &self.entropy
    }

    pub fn list(&self) -> WordList {
        self.list
    }

    /// The words in order, written as a person reads them (Korean in composed letters).
    pub fn words(&self) -> Vec<String> {
        let check = sha256(&self.entropy);
        let entropy_bits = self.entropy.len() * 8;
        let total = entropy_bits + entropy_bits / 32;
        let at = |place: usize| if place < entropy_bits { bit(&self.entropy, place) } else { bit(&check, place - entropy_bits) };
        let index = |word: usize| (0..11).fold(0usize, |index, step| index << 1 | usize::from(at(word * 11 + step)));
        let word = |number: usize| self.list.word(index(number)).unwrap_or_default().nfc().collect::<String>();
        (0..total / 11).map(word).collect()
    }

    pub fn phrase(&self) -> String {
        self.words().join(" ")
    }

    /// BIP 39's seed: PBKDF2 with HMAC-SHA512 over the words, 2,048 rounds.
    pub fn seed(&self, passphrase: &str) -> [u8; 64] {
        let sentence: String = self.phrase().nfkd().collect();
        let mut salt = "mnemonic".to_string();
        salt.extend(passphrase.nfkd());
        let mut seed = [0u8; 64];
        pbkdf2::pbkdf2_hmac::<sha2::Sha512>(sentence.as_bytes(), salt.as_bytes(), ROUNDS, &mut seed);
        seed
    }
}

fn child(key: &XPrv, index: u32, hardened: bool) -> Result<XPrv, SeedError> {
    let number = ChildNumber::new(index, hardened).map_err(|_| SeedError::BadKey)?;
    key.derive_child(number).map_err(|_| SeedError::BadKey)
}

/// The secret key at m/44'/5196867'/`account`'/0/`index`.
pub fn derive_secret(seed: &[u8; 64], account: u32, index: u32) -> Result<[u8; 32], SeedError> {
    let master = XPrv::new(seed).map_err(|_| SeedError::BadKey)?;
    let account_key = [(44, true), (COIN_TYPE, true), (account, true), (0, false)]
        .into_iter()
        .try_fold(master, |key, (number, hardened)| child(&key, number, hardened))?;
    let key = child(&account_key, index, false)?;
    Ok(key.private_key().to_bytes().into())
}

/// The addresses of `count` keys from `from` on, to look them up before taking them in.
pub fn addresses(seed: &[u8; 64], from: u32, count: u32) -> Result<Vec<olc_core::Address>, SeedError> {
    let address = |index: u32| {
        let secret = derive_secret(seed, 0, index)?;
        olc_crypto::public_key(&secret).map(|public| olc_crypto::address_of(&public)).ok_or(SeedError::BadKey)
    };
    (from..from.saturating_add(count)).map(address).collect()
}

/// Keys past the last one with coins that a restore still looks at, as Bitcoin wallets do.
pub const GAP: u32 = 20;

impl Mnemonic {
    fn record(&self, next: u32, written: bool) -> SeedRecord {
        SeedRecord { entropy: self.entropy.clone(), korean: self.list == WordList::Korean, next, written }
    }

    fn key(&self, index: u32) -> Result<[u8; 32], SeedError> {
        derive_secret(&self.seed(""), 0, index)
    }
}

impl SeedRecord {
    pub fn mnemonic(&self) -> Option<Mnemonic> {
        let list = if self.korean { WordList::Korean } else { WordList::English };
        Mnemonic::from_entropy(&self.entropy, list).ok()
    }
}

impl Wallet {
    /// A new wallet whose first key comes from these words. `written` is false until the person
    /// has written them down.
    pub fn from_words(words: &Mnemonic, written: bool) -> Result<Self, SeedError> {
        let mut wallet = Self::from_secret(words.key(0)?).map_err(|_| SeedError::BadKey)?;
        wallet.seed = Some(words.record(1, written));
        Ok(wallet)
    }

    /// Gives an older wallet seed words: their first key joins and receives from now on, and
    /// every key it held stays.
    pub fn add_words(&mut self, words: &Mnemonic) -> Result<usize, SeedError> {
        let index = self.add_secret(words.key(0)?).map_err(|_| SeedError::BadKey)?;
        self.receiving = index;
        self.seed = Some(words.record(1, false));
        Ok(index)
    }

    /// The next key from the words, when there are words.
    pub fn next_word_key(&mut self) -> Option<Result<usize, SeedError>> {
        let record = self.seed.clone()?;
        let words = record.mnemonic()?;
        let added = words.key(record.next).and_then(|secret| self.add_secret(secret).map_err(|_| SeedError::BadKey));
        if let Some(seed) = self.seed.as_mut() {
            seed.next += 1;
        }
        Some(added)
    }

    /// Takes in the keys these words give: the first, and every one up to the last that `used`
    /// says has coins, looking `GAP` past it. The first becomes the one that receives, so this
    /// computer shows the address the words were made with. Says how many keys were new.
    pub fn restore_words(&mut self, words: &Mnemonic, used: impl Fn(&olc_core::Address) -> bool) -> Result<usize, SeedError> {
        let seed = words.seed("");
        let mut secrets = Vec::new();
        let mut last_used = 0;
        let mut index = 0;
        while index <= last_used + GAP {
            let secret = derive_secret(&seed, 0, index)?;
            let public = olc_crypto::public_key(&secret).ok_or(SeedError::BadKey)?;
            if used(&olc_crypto::address_of(&public)) {
                last_used = index;
            }
            secrets.push(secret);
            index += 1;
        }
        secrets.truncate(last_used as usize + 1);
        let added = secrets.iter().filter(|secret| self.add_secret(**secret).is_ok()).count();
        let first = olc_crypto::public_key(&secrets[0]).map(|public| olc_crypto::address_of(&public));
        self.receiving = self.keys.iter().position(|key| Some(key.address) == first).unwrap_or(self.receiving);
        self.seed = Some(words.record(last_used + 1, true));
        Ok(added)
    }
}

#[cfg(test)]
#[path = "seed_tests.rs"]
mod tests;
