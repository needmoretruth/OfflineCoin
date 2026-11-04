use super::*;
use crate::wallet_file::{back_up, load_wallet, restore, save_wallet, NEEDS_PASSWORD, WRONG_PASSWORD};
use olc_wallet::seed::{Mnemonic, WordList};
use std::path::PathBuf;

const PASSWORD: &str = "long horse battery";

fn folder(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("olc-sealed-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).unwrap();
    path
}

fn words() -> Mnemonic {
    Mnemonic::from_entropy(&[9; 16], WordList::English).unwrap()
}

/// Words, a key from before them, a name and a contact: everything a wallet file holds.
fn wallet() -> Wallet {
    let mut wallet = Wallet::from_words(&words(), true).unwrap();
    wallet.add_secret([4; 32]).unwrap();
    wallet.labels.insert("olc0303".into(), "Rent".into());
    wallet.contacts.push(Contact { name: "Mina".into(), address: "olc1mina".into() });
    wallet
}

fn sealed_in(name: &str) -> (PathBuf, Wallet) {
    let directory = folder(name);
    let wallet = wallet();
    save_wallet(&directory, &wallet).unwrap();
    seal_wallet(&directory, &wallet, PASSWORD).unwrap();
    (directory, wallet)
}

fn file_bytes(directory: &Path) -> Vec<u8> {
    fs::read(directory.join(WALLET_FILE)).unwrap()
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|window| window == needle)
}

/// Every secret in every shape a file could hold it: raw bytes, hex either case, and the words.
fn secret_shapes(wallet: &Wallet) -> Vec<Vec<u8>> {
    let mut shapes = Vec::new();
    for index in 0..wallet.key_count() {
        let secret = wallet.export_secret(index).unwrap();
        shapes.push(secret.to_vec());
        shapes.push(hex(&secret).into_bytes());
        shapes.push(hex(&secret).to_uppercase().into_bytes());
    }
    let seed = wallet.seed.as_ref().unwrap();
    shapes.push(seed.entropy.clone());
    shapes.push(hex(&seed.entropy).into_bytes());
    shapes.push(words().phrase().into_bytes());
    shapes
}

#[test]
fn a_sealed_wallet_file_holds_no_secret_key_bytes() {
    let (directory, wallet) = sealed_in("no-secrets");
    let bytes = file_bytes(&directory);
    for shape in secret_shapes(&wallet) {
        assert!(!contains(&bytes, &shape), "a secret is readable in the sealed file");
    }
    // The plain file it replaced held them, which is what the scan is there to catch.
    let plain = folder("no-secrets-plain");
    save_wallet(&plain, &wallet).unwrap();
    assert!(contains(&file_bytes(&plain), hex(&wallet.export_secret(1).unwrap()).as_bytes()));
    let _ = fs::remove_dir_all(plain);
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn a_wrong_password_changes_nothing() {
    let (directory, _) = sealed_in("wrong");
    let before = file_bytes(&directory);
    assert_eq!(open_wallet(&directory, "long horse").err().as_deref(), Some(WRONG_WALLET_PASSWORD));
    assert_eq!(change_password(&directory, "wrong", "new one").err().as_deref(), Some(WRONG_WALLET_PASSWORD));
    assert_eq!(seal_wallet(&directory, &wallet(), "other").err().as_deref(), Some(ALREADY_SEALED));
    assert_eq!(file_bytes(&directory), before);
    fs::remove_dir_all(directory).unwrap();
}

/// After a restart the wallet is locked: it knows its addresses and nothing more until the
/// password opens it, and then it has every key and its words back.
#[test]
fn a_sealed_wallet_starts_locked_and_opens_with_its_password() {
    let (directory, wallet) = sealed_in("restart");
    let locked = load_wallet(&directory).unwrap();
    assert!(!locked.is_open() && !locked.holds_secrets());
    assert_eq!(locked.keys().collect::<Vec<_>>(), wallet.keys().collect::<Vec<_>>());
    assert_eq!(locked.address(), wallet.address());
    assert!(locked.seed.is_none());
    assert_eq!(locked.contacts, wallet.contacts);
    assert_eq!(sealed_words(&directory), Some(SealedWords { has: true, written: true }));
    let (open, _) = open_wallet(&directory, PASSWORD).unwrap();
    assert!(open.is_open());
    assert_eq!(open.export_secret(1), Some([4; 32]));
    assert_eq!(open.seed, wallet.seed);
    fs::remove_dir_all(directory).unwrap();
}

/// Names and contacts change while locked. Keys never do, and a save that would lose or bare
/// one is refused.
#[test]
fn a_locked_wallet_saves_names_and_contacts_and_never_its_keys() {
    let (directory, _) = sealed_in("locked-save");
    let mut locked = load_wallet(&directory).unwrap();
    locked.contacts.push(Contact { name: "Jun".into(), address: "olc1jun".into() });
    locked.set_receiving(1).unwrap();
    save_wallet(&directory, &locked).unwrap();
    let (open, seal) = open_wallet(&directory, PASSWORD).unwrap();
    assert_eq!(open.contacts.len(), 2);
    assert_eq!(open.receiving(), 1);
    assert_eq!(open.export_secret(1), Some([4; 32]));
    // An open wallet saved without its seal would write keys in the clear: refused.
    assert_eq!(save_wallet(&directory, &open).err().as_deref(), Some(WALLET_LOCKED));
    let mut more = open.clone();
    more.add_secret([5; 32]).unwrap();
    save_open(&directory, &more, &seal).unwrap();
    assert_eq!(load_wallet(&directory).unwrap().key_count(), 3);
    // A locked copy from before the new key would drop it: refused.
    assert_eq!(save_wallet(&directory, &locked).err().as_deref(), Some(WALLET_LOCKED));
    assert_eq!(open_wallet(&directory, PASSWORD).unwrap().0.export_secret(2), Some([5; 32]));
    fs::remove_dir_all(directory).unwrap();
}

/// A locked wallet hands out new receiving addresses from its words' account, the same ones the
/// words make, with no secret in the file; opening it gives each one's secret from the words. A
/// key the account did not make is still refused.
#[test]
fn a_locked_wallet_makes_receiving_addresses_the_words_open() {
    let (directory, wallet) = sealed_in("locked-receive");
    let mut locked = load_wallet(&directory).unwrap();
    let index = locked.next_word_key().unwrap().unwrap();
    locked.set_receiving(index).unwrap();
    save_wallet(&directory, &locked).unwrap();
    let mut same = wallet.clone();
    let from_words = same.next_word_key().unwrap().unwrap();
    assert_eq!(locked.address(), same.keys().nth(from_words).unwrap().0, "the address the words give");
    let bytes = file_bytes(&directory);
    for shape in secret_shapes(&same) {
        assert!(!contains(&bytes, &shape), "a secret is readable in the sealed file");
    }
    let mut restarted = load_wallet(&directory).unwrap();
    assert_eq!(restarted.address(), locked.address(), "the new address outlives a restart");
    restarted.next_word_key().unwrap().unwrap();
    save_wallet(&directory, &restarted).unwrap();
    let (open, _) = open_wallet(&directory, PASSWORD).unwrap();
    assert!(open.is_open(), "every address handed out has its secret");
    assert_eq!(open.address(), locked.address());
    assert_eq!(open.seed.as_ref().map(|seed| seed.next), Some(3), "the words count on from there");
    let mut publics: Vec<_> = load_wallet(&directory).unwrap().keys().map(|(_, public)| public).collect();
    publics.push(Wallet::from_secret([8; 32]).unwrap().keys().next().unwrap().1);
    let mut forged = Wallet::from_publics(&publics, 0).unwrap();
    forged.watch = load_wallet(&directory).unwrap().watch;
    assert_eq!(save_wallet(&directory, &forged).err().as_deref(), Some(WALLET_LOCKED));
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn a_backup_made_while_locked_comes_back_with_the_wallet_password() {
    let (directory, wallet) = sealed_in("locked-backup");
    let mut locked = load_wallet(&directory).unwrap();
    assert_eq!(back_up(&locked, &directory.join("plain.backup"), None).err().as_deref(), Some(WALLET_LOCKED));
    locked.contacts.push(Contact { name: "Jun".into(), address: "olc1jun".into() });
    save_wallet(&directory, &locked).unwrap();
    let path = directory.join("locked.backup");
    back_up_locked(&directory, &path).unwrap();
    let bytes = fs::read(&path).unwrap();
    for shape in secret_shapes(&wallet) {
        assert!(!contains(&bytes, &shape), "a secret is readable in the backup");
    }
    assert_eq!(restore(&path, None).err().as_deref(), Some(NEEDS_PASSWORD));
    assert_eq!(restore(&path, Some("wrong")).err().as_deref(), Some(WRONG_PASSWORD));
    let back = restore(&path, Some(PASSWORD)).unwrap();
    assert_eq!(back.export_secret(1), Some([4; 32]));
    assert_eq!(back.seed, wallet.seed);
    assert_eq!(back.contacts.len(), 2, "a contact added while locked travels with the backup");
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn a_new_password_opens_the_wallet_and_the_old_one_stops() {
    let (directory, wallet) = sealed_in("change");
    change_password(&directory, PASSWORD, "second password").unwrap();
    assert_eq!(open_wallet(&directory, PASSWORD).err().as_deref(), Some(WRONG_WALLET_PASSWORD));
    let (open, _) = open_wallet(&directory, "second password").unwrap();
    assert_eq!(open.export_secret(1), wallet.export_secret(1));
    assert_eq!(change_password(&directory, "second password", "").err().as_deref(), Some(EMPTY_PASSWORD));
    fs::remove_dir_all(directory).unwrap();
}

/// A plain wallet stays the file older builds read, byte for byte, until a password seals it.
#[test]
fn a_plain_wallet_file_stays_as_older_builds_wrote_it() {
    let directory = folder("plain-same");
    let older = "{\n  \"keys\": [\n    \"0303030303030303030303030303030303030303030303030303030303030303\"\n  ],\n  \"receiving\": 0\n}";
    fs::write(directory.join(WALLET_FILE), older).unwrap();
    let loaded = load_wallet(&directory).unwrap();
    assert!(loaded.is_open() && loaded.contacts.is_empty());
    save_wallet(&directory, &loaded).unwrap();
    assert_eq!(fs::read_to_string(directory.join(WALLET_FILE)).unwrap(), older);
    assert_eq!(sealed_words(&directory), None);
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn an_older_nodes_key_file_goes_once_its_key_is_sealed() {
    let directory = folder("old-key");
    fs::write(directory.join(OLD_KEY_FILE), hex(&[3; 32])).unwrap();
    let wallet = load_wallet(&directory).unwrap();
    seal_wallet(&directory, &wallet, PASSWORD).unwrap();
    assert!(!directory.join(OLD_KEY_FILE).exists());
    fs::remove_dir_all(directory).unwrap();
}
