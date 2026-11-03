//! Seed words, signed messages and shared wallets: what a person does with their wallet beyond
//! keys. These work on a node's folder as well as on a running node.
use super::ops::Book;
use olc_core::Address;
use olc_store::new_words;
use olc_wallet::message::verify_message;
use olc_wallet::seed::{addresses, Mnemonic, SeedError, WordList, GAP};
use olc_wallet::shared::Shared;
use olc_wallet::{format_address, parse_address, Wallet};
use std::collections::BTreeSet;

/// Something to do with this wallet's words, messages or shared wallets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WalletOrder {
    /// The seed words, for the command line. The app reads them from its view.
    ShowWords,
    /// The person wrote the words down, so the app stops showing them.
    WordsWritten,
    /// Gives a wallet made before seed words its own.
    NewWords { korean: bool },
    /// Takes in the keys of these words.
    RestoreWords(String),
    /// Signs a message with the key of `address`, or the receiving key when it is empty.
    SignMessage { address: String, message: String },
    VerifyMessage { address: String, signature: String, message: String },
    /// A shared wallet of `need` of these public keys, given as hex.
    MakeShared { need: usize, keys: Vec<String> },
    /// A shared wallet someone else made, as its text.
    AddShared(String),
    ForgetShared(usize),
    ListShared,
}

pub const NO_WORDS: &str = "notice.no_words";

/// Why words did not read, as a notice's key and its detail.
pub fn words_fault(error: &SeedError) -> (&'static str, String) {
    match error {
        SeedError::Count(count) => ("notice.words_count", count.to_string()),
        SeedError::Unknown(place) => ("notice.words_unknown", place.to_string()),
        SeedError::Checksum => ("notice.words_checksum", String::new()),
        SeedError::BadKey => ("notice.words_checksum", String::new()),
    }
}

/// Every failure here, so [`wallet_failure`] can read one back as a notice.
const FAILURES: [&str; 10] = [
    "notice.no_words",
    "notice.has_words",
    "notice.words_count",
    "notice.words_unknown",
    "notice.words_checksum",
    "notice.bad_address",
    "notice.no_such_key",
    "notice.message_bad",
    "notice.shared_bad",
    "notice.shared_not_mine",
];

/// A failure of this file's, given back as `key` or `key:detail`, as its notice.
pub fn wallet_failure(named: &str) -> Option<(&'static str, String)> {
    let (key, detail) = named.split_once(':').unwrap_or((named, ""));
    FAILURES.iter().find(|known| **known == key).map(|key| (*key, detail.to_string()))
}

/// A notice's key and detail as one failure, read back by [`wallet_failure`].
fn fault(error: &SeedError) -> String {
    let (key, detail) = words_fault(error);
    if detail.is_empty() {
        key.to_string()
    } else {
        format!("{key}:{detail}")
    }
}

fn address_or_receiving(wallet: &Wallet, text: &str) -> Result<Address, String> {
    match text.trim() {
        "" => Ok(wallet.address()),
        text => parse_address(text).ok_or_else(|| "notice.bad_address".to_string()),
    }
}

/// Which addresses of these words hold coins on the running node, looking [`GAP`] past the
/// last one that does. Without a node, none.
fn used_addresses<B: Book + ?Sized>(book: &B, words: &Mnemonic) -> Result<BTreeSet<Address>, String> {
    let seed = words.seed("");
    let mut used = BTreeSet::new();
    let mut looked = 0u32;
    let mut until = GAP + 1;
    while looked < until {
        let batch = addresses(&seed, looked, until - looked).map_err(|error| fault(&error))?;
        let held = book.live(|runner| {
            let coins = runner.engine.coins();
            batch.iter().map(|address| !coins.unspent_of(address).is_empty()).collect::<Vec<bool>>()
        })?;
        for (offset, has) in held.unwrap_or_default().into_iter().enumerate() {
            if has {
                used.insert(batch[offset]);
                until = until.max(looked + offset as u32 + GAP + 1);
            }
        }
        looked += batch.len() as u32;
    }
    Ok(used)
}

/// One line for each shared wallet: its number, who must sign, what it holds and its text.
fn list_shared<B: Book + ?Sized>(book: &B) -> Result<String, String> {
    let shared = book.wallet_view(|wallet| wallet.shared.clone())?;
    let rows = shared.iter().enumerate().map(|(index, shared)| {
        let held = book.live(|runner| runner.engine.shared_coins(shared).iter().map(|(_, value)| value).sum::<u64>());
        let held = held.ok().flatten().map_or_else(|| "?".to_string(), olc_core::amount::format_coins);
        format!("{index}  {} of {}  {held}  {}", shared.need, shared.keys.len(), shared.to_text())
    });
    let rows: Vec<String> = rows.collect();
    Ok(if rows.is_empty() { "No shared wallets yet.".to_string() } else { rows.join("\n") })
}

/// Does a wallet order and gives the notice's key and detail.
pub(super) fn carry_wallet<B: Book + ?Sized>(book: &B, order: WalletOrder) -> Result<(&'static str, String), String> {
    match order {
        WalletOrder::ShowWords => {
            let words = book.wallet_view(|wallet| wallet.seed.as_ref().and_then(|seed| seed.mnemonic()))?;
            Ok(("notice.words", words.ok_or(NO_WORDS)?.phrase()))
        }
        WalletOrder::WordsWritten => book.wallet_change(|wallet| {
            let seed = wallet.seed.as_mut().ok_or(NO_WORDS)?;
            seed.written = true;
            Ok(("notice.words_written", String::new()))
        }),
        WalletOrder::NewWords { korean } => {
            let list = if korean { WordList::Korean } else { WordList::English };
            let words = new_words(list)?;
            book.wallet_change(|wallet| {
                if wallet.seed.is_some() {
                    return Err("notice.has_words".to_string());
                }
                wallet.add_words(&words).map_err(|error| fault(&error))?;
                Ok(("notice.words_made", format_address(&wallet.address())))
            })
        }
        WalletOrder::RestoreWords(text) => {
            let words = Mnemonic::parse(&text).map_err(|error| fault(&error))?;
            let used = used_addresses(book, &words)?;
            book.wallet_change(|wallet| {
                wallet.restore_words(&words, |address| used.contains(address)).map_err(|error| fault(&error))?;
                Ok(("notice.words_restored", format_address(&wallet.address())))
            })
        }
        WalletOrder::SignMessage { address, message } => {
            let signed = book.wallet_view(|wallet| {
                let address = address_or_receiving(wallet, &address)?;
                wallet.sign_message(&address, &message).ok_or_else(|| "notice.no_such_key".to_string())
            })??;
            Ok(("notice.message_signed", signed))
        }
        WalletOrder::VerifyMessage { address, signature, message } => {
            let address = parse_address(address.trim()).ok_or("notice.bad_address")?;
            match verify_message(&address, &message, &signature) {
                true => Ok(("notice.message_good", format_address(&address))),
                false => Err("notice.message_bad".to_string()),
            }
        }
        WalletOrder::MakeShared { need, keys } => {
            let keys: Option<Vec<Vec<u8>>> = keys.iter().map(|key| olc_lang::words::public_key_from_text(key).map(|key| key.to_vec())).collect();
            let shared = Shared::new(need, keys.ok_or("notice.shared_bad")?).map_err(|_| "notice.shared_bad")?;
            add_shared(book, shared)
        }
        WalletOrder::AddShared(text) => add_shared(book, Shared::from_text(&text).map_err(|_| "notice.shared_bad")?),
        WalletOrder::ForgetShared(index) => book.wallet_change(|wallet| {
            if index >= wallet.shared.len() {
                return Err("notice.shared_bad".to_string());
            }
            wallet.shared.remove(index);
            Ok(("notice.saved", String::new()))
        }),
        WalletOrder::ListShared => Ok(("notice.keys", list_shared(book)?)),
    }
}

fn add_shared<B: Book + ?Sized>(book: &B, shared: Shared) -> Result<(&'static str, String), String> {
    let text = shared.to_text();
    book.wallet_change(|wallet| {
        if shared.held_by(wallet).is_empty() {
            return Err("notice.shared_not_mine".to_string());
        }
        if !wallet.shared.contains(&shared) {
            wallet.shared.push(shared);
        }
        Ok(("notice.shared_made", text))
    })
}
