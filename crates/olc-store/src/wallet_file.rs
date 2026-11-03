//! A wallet on disk, and its backups. A backup with a password goes through Argon2id and
//! ChaCha20-Poly1305 from audited crates. Nothing here prints a key.
use argon2::Argon2;
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use olc_platform::platform;
use olc_wallet::seed::{Mnemonic, WordList};
use olc_wallet::shared::Shared;
use olc_wallet::{SeedRecord, Wallet};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::ErrorKind;
use std::path::Path;

const DAMAGED_WALLET: &str = "The wallet file is damaged.";
const WALLET_FILE: &str = "wallet.json";
const OLD_KEY_FILE: &str = "wallet.key";

#[derive(Serialize, Deserialize)]
struct Keys {
    keys: Vec<String>,
    receiving: usize,
    /// Older builds read past these two.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    seed: Option<SeedRecord>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    shared: Vec<Shared>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    earlier: Vec<SeedRecord>,
}

/// What a backup file holds. With a password only the sealed fields are set.
#[derive(Serialize, Deserialize)]
struct Backup {
    olc_wallet_backup: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    keys: Option<Keys>,
    #[serde(skip_serializing_if = "Option::is_none")]
    salt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    nonce: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sealed: Option<String>,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.is_ascii() || !text.len().is_multiple_of(2) {
        return None;
    }
    let pair = |index| u8::from_str_radix(text.get(index..index + 2)?, 16).ok();
    (0..text.len()).step_by(2).map(pair).collect()
}

/// Reads a secret key written as hex.
pub fn parse_secret(text: &str) -> Option<[u8; 32]> {
    unhex(text.trim())?.try_into().ok()
}

pub fn format_secret(secret: &[u8; 32]) -> String {
    hex(secret)
}

fn keys_of(wallet: &Wallet) -> Keys {
    let secrets = (0..wallet.key_count()).filter_map(|index| wallet.export_secret(index));
    Keys {
        keys: secrets.map(|secret| hex(&secret)).collect(),
        receiving: wallet.receiving(),
        seed: wallet.seed.clone(),
        shared: wallet.shared.clone(),
        earlier: wallet.earlier.clone(),
    }
}

fn wallet_of(keys: &Keys) -> Result<Wallet, String> {
    let parsed: Option<Vec<[u8; 32]>> = keys.keys.iter().map(|text| parse_secret(text)).collect();
    let secrets = parsed.ok_or("A key in the wallet file is damaged.")?;
    let mut wallet = Wallet::from_secrets(&secrets, keys.receiving).map_err(|_| "A key is not valid.".to_string())?;
    wallet.seed = keys.seed.clone();
    wallet.shared = keys.shared.clone();
    wallet.earlier = keys.earlier.clone();
    Ok(wallet)
}

/// Writes through a new file, so a cut leaves the old one whole. Only the owner can read it.
fn write_private(path: &Path, text: &str) -> Result<(), String> {
    let temporary = path.with_extension("new");
    let _ = fs::remove_file(&temporary);
    platform().write_private(&temporary, text.as_bytes()).map_err(|error| error.to_string())?;
    fs::rename(&temporary, path).map_err(|error| error.to_string())
}

fn random<const SIZE: usize>() -> Result<[u8; SIZE], String> {
    let mut bytes = [0u8; SIZE];
    getrandom::fill(&mut bytes).map_err(|error| error.to_string())?;
    Ok(bytes)
}

/// A new secret key from the system's randomness.
pub fn new_secret() -> Result<[u8; 32], String> {
    random()
}

/// New seed words from the system's randomness.
pub fn new_words(list: WordList) -> Result<Mnemonic, String> {
    let entropy: [u8; 16] = random()?;
    Mnemonic::from_entropy(&entropy, list).map_err(|_| "No words could be made.".to_string())
}

/// Opens the wallet in `directory`, making one from new seed words the first time. An older
/// node's single key file becomes the wallet's first key.
pub fn load_wallet(directory: &Path) -> Result<Wallet, String> {
    let path = directory.join(WALLET_FILE);
    match fs::read_to_string(&path) {
        Ok(text) => {
            let keys: Keys = serde_json::from_str(&text).map_err(|_| DAMAGED_WALLET)?;
            return wallet_of(&keys);
        }
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        // Anything else means keys may be there. Writing a new wallet would put an end to them.
        Err(_) => return Err(DAMAGED_WALLET.to_string()),
    }
    fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    let old = fs::read_to_string(directory.join(OLD_KEY_FILE)).ok();
    let wallet = match old {
        Some(text) => {
            let secret = parse_secret(&text).ok_or("The key file holds no valid key.")?;
            Wallet::from_secret(secret).map_err(|_| "The key is not valid.")?
        }
        None => Wallet::from_words(&new_words(WordList::English)?, false).map_err(|_| "The key is not valid.")?,
    };
    save_wallet(directory, &wallet)?;
    Ok(wallet)
}

pub fn save_wallet(directory: &Path, wallet: &Wallet) -> Result<(), String> {
    let text = serde_json::to_string_pretty(&keys_of(wallet)).map_err(|error| error.to_string())?;
    write_private(&directory.join(WALLET_FILE), &text)
}

pub const WRONG_PASSWORD: &str = "Wrong password, or the backup is damaged.";
pub const NEEDS_PASSWORD: &str = "This backup needs its password.";
pub const NOT_A_BACKUP: &str = "This is not a wallet backup.";

fn password_key(password: &str, salt: &[u8]) -> Result<[u8; 32], String> {
    let mut key = [0u8; 32];
    Argon2::default()
        .hash_password_into(password.as_bytes(), salt, &mut key)
        .map_err(|error| error.to_string())?;
    Ok(key)
}

/// Writes a backup of every key to `path`, sealed when there is a password.
pub fn back_up(wallet: &Wallet, path: &Path, password: Option<&str>) -> Result<(), String> {
    let keys = keys_of(wallet);
    let mut backup = Backup {
        olc_wallet_backup: 1,
        keys: None,
        salt: None,
        nonce: None,
        sealed: None,
    };
    match password.filter(|password| !password.is_empty()) {
        None => backup.keys = Some(keys),
        Some(password) => {
            let salt: [u8; 16] = random()?;
            let nonce: [u8; 12] = random()?;
            let key = password_key(password, &salt)?;
            let plain = serde_json::to_vec(&keys).map_err(|error| error.to_string())?;
            let sealed = ChaCha20Poly1305::new(&Key::from(key))
                .encrypt(&Nonce::from(nonce), plain.as_slice())
                .map_err(|_| "The backup could not be sealed.")?;
            backup.salt = Some(hex(&salt));
            backup.nonce = Some(hex(&nonce));
            backup.sealed = Some(hex(&sealed));
        }
    }
    let text = serde_json::to_string_pretty(&backup).map_err(|error| error.to_string())?;
    write_private(path, &text)
}

/// Reads the keys of a backup. A wrong password fails here and never gives wrong keys.
pub fn restore(path: &Path, password: Option<&str>) -> Result<Wallet, String> {
    let text = fs::read_to_string(path).map_err(|error| error.to_string())?;
    let backup: Backup = serde_json::from_str(&text).map_err(|_| NOT_A_BACKUP)?;
    if let Some(keys) = &backup.keys {
        return wallet_of(keys);
    }
    let fields = (&backup.salt, &backup.nonce, &backup.sealed);
    let (Some(salt), Some(nonce), Some(sealed)) = fields else {
        return Err(NOT_A_BACKUP.to_string());
    };
    let damaged = || "The backup is damaged.".to_string();
    let salt = unhex(salt).ok_or_else(damaged)?;
    let nonce = unhex(nonce).and_then(|nonce| <[u8; 12]>::try_from(nonce).ok());
    let nonce = nonce.ok_or_else(damaged)?;
    let sealed = unhex(sealed).ok_or_else(damaged)?;
    let key = password_key(password.ok_or(NEEDS_PASSWORD)?, &salt)?;
    let plain = ChaCha20Poly1305::new(&Key::from(key))
        .decrypt(&Nonce::from(nonce), sealed.as_slice())
        .map_err(|_| WRONG_PASSWORD)?;
    let keys: Keys = serde_json::from_slice(&plain).map_err(|_| damaged())?;
    wallet_of(&keys)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One bad sector must not turn into a new wallet and the end of the old keys.
    #[test]
    fn a_wallet_file_that_cannot_be_read_stays_as_it_is() {
        let directory = folder("unreadable");
        let path = directory.join(WALLET_FILE);
        let damaged = [0xFF, 0xFE, 0x00, 0x7B];
        fs::write(&path, damaged).unwrap();
        assert!(load_wallet(&directory).is_err());
        assert_eq!(fs::read(&path).unwrap(), damaged);
    }

    fn folder(name: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("olc-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn wallet() -> Wallet {
        let mut wallet = Wallet::from_secret([3; 32]).unwrap();
        wallet.add_secret([4; 32]).unwrap();
        wallet.set_receiving(1).unwrap();
        wallet
    }

    #[test]
    fn a_sealed_backup_comes_back_only_with_its_password() {
        let directory = folder("sealed");
        let path = directory.join("backup.json");
        back_up(&wallet(), &path, Some("long horse")).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(!text.contains(&hex(&[3; 32])));
        let back = restore(&path, Some("long horse")).unwrap();
        assert_eq!(back.address(), wallet().address());
        assert_eq!(back.key_count(), 2);
        assert!(restore(&path, Some("wrong")).is_err());
        assert!(restore(&path, None).is_err());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_plain_backup_and_the_wallet_file_keep_every_key() {
        let directory = folder("plain");
        let path = directory.join("backup.json");
        back_up(&wallet(), &path, None).unwrap();
        assert_eq!(restore(&path, None).unwrap().address(), wallet().address());
        save_wallet(&directory, &wallet()).unwrap();
        assert_eq!(load_wallet(&directory).unwrap().address(), wallet().address());
        fs::remove_dir_all(directory).unwrap();
    }

    /// Words a person writes down give back the wallet's own address, through its file too.
    #[test]
    fn a_new_wallet_comes_from_words_that_give_its_address_back() {
        let directory = folder("words");
        let made = load_wallet(&directory).unwrap();
        let record = made.seed.clone().unwrap();
        assert!(!record.written);
        let words = record.mnemonic().unwrap();
        assert_eq!(words.words().len(), 12);
        let elsewhere = Wallet::from_words(&Mnemonic::parse(&words.phrase()).unwrap(), true).unwrap();
        assert_eq!(elsewhere.address(), made.address());
        assert_eq!(load_wallet(&directory).unwrap().seed, Some(record));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn an_older_nodes_key_becomes_the_first_key() {
        let directory = folder("old");
        fs::write(directory.join(OLD_KEY_FILE), hex(&[3; 32])).unwrap();
        let loaded = load_wallet(&directory).unwrap();
        assert_eq!(loaded.address(), Wallet::from_secret([3; 32]).unwrap().address());
        fs::remove_dir_all(directory).unwrap();
    }
}
