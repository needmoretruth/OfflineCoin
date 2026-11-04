//! Seed words, signed messages and shared wallets: what a person does with their wallet beyond
//! keys. These work on a node's folder as well as on a running node.
use super::ops::Book;
use super::wallet_contacts::ContactOrder;
use super::wallet_lock::PasswordOrder;
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
    /// Pays each `(address, amount)` in one payment at `rate` units a byte, from the coins in
    /// `coins` (as `id:number`) when it names any. `label` names the payment.
    Pay { to: Vec<(String, String)>, rate: u64, coins: Vec<String>, label: String },
    /// A new address to receive with, which then receives.
    NewAddress,
    /// Names an address or a payment id. An empty name takes the name away.
    Label { of: String, name: String },
    /// Stops following an address watched without its key.
    Unwatch(String),
    /// Seals, opens, locks or changes the wallet's password.
    Password(PasswordOrder),
    /// Adds, renames, removes or lists other people's addresses.
    Contact(ContactOrder),
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
const FAILURES: &[&str] = &[
    "notice.wallet_locked",
    "notice.wrong_wallet_password",
    "notice.already_sealed",
    "notice.not_sealed",
    "notice.empty_password",
    "notice.node_on_wallet",
    "notice.unlock_needs_node",
    "notice.contact_name_bad",
    "notice.contact_name_taken",
    "notice.no_contact",
    "notice.bad_amount",
    "notice.not_enough",
    "notice.pay_refused",
    "notice.zero_amount",
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

/// Does a wallet order on the wallet files in `directory`, as before the first run's questions
/// are done, when no node runs. Gives the notice's key, detail and whether it went well.
pub(super) fn carry_in_folder(directory: std::path::PathBuf, order: WalletOrder) -> (&'static str, String, bool) {
    match carry_wallet(&super::folder::Folder { directory }, order) {
        Ok((key, detail)) => (key, detail, true),
        Err(error) => {
            let (key, detail) = wallet_failure(&error).unwrap_or(("notice.key_failed", error));
            (key, detail, false)
        }
    }
}

/// Does a wallet order and gives the notice's key and detail.
pub(super) fn carry_wallet<B: Book + ?Sized>(book: &B, order: WalletOrder) -> Result<(&'static str, String), String> {
    match order {
        WalletOrder::ShowWords => {
            let (words, earlier, open) = book.wallet_view(|wallet| {
                let earlier: Vec<String> = wallet.earlier.iter().filter_map(|record| record.mnemonic()).map(|words| words.phrase()).collect();
                (wallet.seed.as_ref().and_then(|seed| seed.mnemonic()), earlier, wallet.is_open())
            })?;
            // A locked wallet's words are sealed with its keys.
            if !open {
                return Err(LOCKED.to_string());
            }
            let mut shown = words.ok_or(NO_WORDS)?.phrase();
            // Words this wallet had before still bring their coins back, so they show too.
            for phrase in earlier {
                shown.push_str("\nEarlier words: ");
                shown.push_str(&phrase);
            }
            Ok(("notice.words", shown))
        }
        WalletOrder::WordsWritten => book.wallet_change(|wallet| {
            if !wallet.is_open() {
                return Err(LOCKED.to_string());
            }
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
                if !wallet.is_open() {
                    return Err(LOCKED.to_string());
                }
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
        WalletOrder::Pay { to, rate, coins, label } => pay(book, &to, rate, &coins, &label),
        WalletOrder::NewAddress => book.wallet_change(|wallet| {
            let index = match wallet.next_word_key() {
                Some(made) => made.map_err(|_| "notice.no_such_key".to_string())?,
                None => wallet.add_secret(olc_store::new_secret()?).map_err(|_| "notice.no_such_key".to_string())?,
            };
            wallet.set_receiving(index).map_err(|_| "notice.no_such_key".to_string())?;
            Ok(("notice.new_address", format_address(&wallet.address())))
        }),
        WalletOrder::Label { of, name } => book.wallet_change(|wallet| {
            let (of, name) = (of.trim().to_string(), name.trim().chars().take(MOST_LABEL).collect::<String>());
            match name.is_empty() {
                true => wallet.labels.remove(&of),
                false => wallet.labels.insert(of, name),
            };
            Ok(("notice.saved", String::new()))
        }),
        WalletOrder::Unwatch(text) => {
            let address = parse_address(text.trim()).ok_or("notice.bad_address")?;
            book.live(|runner| runner.engine.unwatch(&address))?;
            book.change_settings(|settings| settings.watched.retain(|known| parse_address(known) != Some(address)))?;
            Ok(("notice.saved", String::new()))
        }
        WalletOrder::Password(order) => book.password_order(&order),
        WalletOrder::Contact(order) => super::wallet_contacts::carry_contact(book, order),
    }
}

/// What every order that needs the keys says while the wallet is locked.
const LOCKED: &str = "notice.wallet_locked";

/// A name is a few words, as Bitcoin Core's label field is.
const MOST_LABEL: usize = 60;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn coin_named(text: &str) -> Option<olc_core::OutPoint> {
    let (id, index) = text.trim().split_once(':')?;
    Some(olc_core::OutPoint { txid: super::explorer::parse_hash(id)?, index: index.parse().ok()? })
}

/// Pays several people in one payment from the running node, and names it.
fn pay<B: Book + ?Sized>(book: &B, to: &[(String, String)], rate: u64, coins: &[String], label: &str) -> Result<(&'static str, String), String> {
    let mut payments = Vec::with_capacity(to.len());
    for (address, amount) in to {
        let address = parse_address(address.trim()).ok_or("notice.bad_address")?;
        let units = olc_core::amount::parse_coins(amount.trim()).filter(|units| *units > 0).ok_or("notice.bad_amount")?;
        payments.push((address, units));
    }
    let only: Vec<olc_core::OutPoint> = coins.iter().filter_map(|coin| coin_named(coin)).collect();
    let total = payments.iter().map(|(_, units)| *units).fold(0, u64::saturating_add);
    let sent = book.live(|runner| {
        let paid = runner.engine.pay_with(&payments, rate, &only);
        // The fee it paid shows on its waiting row in the history.
        paid.map(|(id, _)| (id, super::host_wallet::amount_of(&runner.engine, total)))
    })?;
    let (id, amount) = sent.ok_or("notice.not_enough")?.map_err(|error| super::host_locks::wallet_key(&error).to_string())?;
    if !label.trim().is_empty() {
        let (of, name) = (hex(&id), label.trim().chars().take(MOST_LABEL).collect::<String>());
        book.wallet_change(|wallet| {
            wallet.labels.insert(of, name);
            Ok(())
        })?;
    }
    Ok(("notice.sent", amount))
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
