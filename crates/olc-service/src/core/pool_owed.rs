//! What the pool owes its workers, kept in a file beside the wallet that pays it. A ripe block's
//! payment waits while the wallet is locked, and the app may close or the pool stop before the
//! wallet opens: the pool forgets its blocks then, and this file remembers what they owe.
use olc_node::Engine;
use olc_pool::{pay_owed, Owed, Pool};
use olc_store::replace_file;
use olc_wallet::{format_address, parse_address};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

const OWED_FILE: &str = "pool_owed.json";
/// The notice given once each time payouts start waiting for the wallet.
pub const WAITS_FOR_WALLET: &str = "notice.pool_waits_wallet";

#[derive(Serialize, Deserialize)]
struct OwedFile {
    olc_pool_owed: u32,
    owed: Vec<OwedRow>,
}

#[derive(Serialize, Deserialize)]
struct OwedRow {
    height: u64,
    block: String,
    fee: u64,
    payments: Vec<(String, u64)>,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn hash_of(text: &str) -> Option<[u8; 32]> {
    let bytes: Option<Vec<u8>> = (0..text.len() / 2).map(|at| u8::from_str_radix(text.get(at * 2..at * 2 + 2)?, 16).ok()).collect();
    bytes?.try_into().ok()
}

fn row_of(owed: &Owed) -> OwedRow {
    let payments = owed.payments.iter().map(|(address, amount)| (format_address(address), *amount)).collect();
    OwedRow { height: owed.height, block: hex(&owed.block), fee: owed.fee, payments }
}

fn owed_of(row: &OwedRow) -> Option<Owed> {
    let payments: Option<Vec<_>> = row.payments.iter().map(|(text, amount)| Some((parse_address(text)?, *amount))).collect();
    Some(Owed { height: row.height, block: hash_of(&row.block)?, payments: payments?, fee: row.fee })
}

/// What the file in `folder` says is owed; nothing when there is no file.
pub fn read(folder: &Path) -> Vec<Owed> {
    let Ok(text) = fs::read_to_string(folder.join(OWED_FILE)) else {
        return Vec::new();
    };
    let file: Option<OwedFile> = serde_json::from_str(&text).ok();
    file.map(|file| file.owed.iter().filter_map(owed_of).collect()).unwrap_or_default()
}

/// Writes what is owed, or takes the file away once nothing is.
pub fn write(folder: &Path, owed: &[Owed]) -> Result<(), String> {
    let path = folder.join(OWED_FILE);
    if owed.is_empty() {
        return match fs::remove_file(&path) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error.to_string()),
            _ => Ok(()),
        };
    }
    let file = OwedFile { olc_pool_owed: 1, owed: owed.iter().map(row_of).collect() };
    let text = serde_json::to_string_pretty(&file).map_err(|error| error.to_string())?;
    replace_file(&path, text.as_bytes())
}

/// The pool's payouts from one turn to the next: those owed from before a restart or a stop, and
/// whether any waits for the wallet to open.
#[derive(Default)]
pub struct Owing {
    read: bool,
    /// Owed by blocks the running pool no longer knows.
    carried: Vec<Owed>,
    /// What the file holds, written again only when it changes.
    kept: Vec<Owed>,
    /// A ripe payout waits for the wallet to open.
    waits: bool,
    notice: Option<&'static str>,
}

impl Owing {
    /// One payout turn: what is owed from before goes out where it can, then the running pool
    /// pays its blocks, and the file follows what is still owed.
    pub fn turn(&mut self, folder: &Path, engine: &mut Engine, pool: Option<&mut Pool>) {
        if !self.read {
            self.read = true;
            self.carried = read(folder);
            self.kept = self.carried.clone();
        }
        let mut held = pay_owed(engine, &mut self.carried);
        let mut owed = self.carried.clone();
        if let Some(pool) = pool {
            pool.pay(engine);
            held |= pool.waits_for_wallet();
            owed.extend(pool.owed(engine));
        }
        if owed != self.kept && write(folder, &owed).is_ok() {
            self.kept = owed;
        }
        if held && !self.waits {
            self.notice = Some(WAITS_FOR_WALLET);
        }
        self.waits = held;
    }

    /// The running pool stops and forgets its blocks: what they owe stays owed.
    pub fn stopping(&mut self, engine: &Engine, pool: &Pool) {
        self.carried.extend(pool.owed(engine));
    }

    pub fn waits(&self) -> bool {
        self.waits
    }

    /// The notice for a person, once each time payouts start to wait.
    pub fn take_notice(&mut self) -> Option<&'static str> {
        self.notice.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("olc-pool-owed-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    /// What the pool owes reads back as it was written, and once nothing is owed no file stays.
    #[test]
    fn what_is_owed_outlives_a_restart_and_leaves_no_file_once_paid() {
        let folder = folder("restart");
        let owed = vec![Owed { height: 12, block: [7; 32], payments: vec![([2; 20], 4_000), ([3; 20], 9_000)], fee: 300 }];
        write(&folder, &owed).unwrap();
        assert_eq!(read(&folder), owed);
        write(&folder, &[]).unwrap();
        assert!(!folder.join(OWED_FILE).exists());
        assert!(read(&folder).is_empty());
        let _ = fs::remove_dir_all(&folder);
    }

    /// A ripe payout owed from before a restart waits while the wallet is locked, says so once,
    /// stays in the file across another restart, and goes out when the wallet opens.
    #[test]
    fn a_payout_owed_waits_for_the_locked_wallet_across_restarts() {
        let folder = folder("locked");
        let mut coin = olc_core::coin::CoinConfig::olc();
        coin.initial_difficulty = 8;
        coin.rules.coinbase_maturity = 1;
        coin.rules.retarget_interval = 0;
        let start = coin.genesis_time_ms;
        let plugins = olc_core::Plugins { consensus: Box::new(olc_pow::ProofOfWork), languages: olc_lang::all() };
        let mut engine = Engine::new(coin, plugins, olc_wallet::Wallet::from_secret([1; 32]).unwrap());
        for block in 1..=3 {
            while !engine.mine(1_000_000, start + block * 60_000) {}
        }
        let block = engine.chain().id_at(1).unwrap();
        let fee = engine.chain().rules().min_fee;
        let owed = vec![Owed { height: 1, block, payments: vec![([2; 20], 5_000)], fee }];
        write(&folder, &owed).unwrap();
        let open = engine.wallet().clone();
        *engine.wallet_mut() = open.locked();

        let mut owing = Owing::default();
        owing.turn(&folder, &mut engine, None);
        assert!(owing.waits(), "the locked wallet holds the payout back");
        assert_eq!(owing.take_notice(), Some(WAITS_FOR_WALLET));
        owing.turn(&folder, &mut engine, None);
        assert_eq!(owing.take_notice(), None, "said once, not every turn");
        assert_eq!(read(&folder), owed, "still owed in the file");

        let mut restarted = Owing::default();
        restarted.turn(&folder, &mut engine, None);
        assert!(restarted.waits(), "a restart keeps it waiting");
        *engine.wallet_mut() = open;
        restarted.turn(&folder, &mut engine, None);
        assert!(!restarted.waits(), "the opened wallet pays it");
        assert!(read(&folder).is_empty() && !folder.join(OWED_FILE).exists(), "nothing owed, no file");
        assert_eq!(engine.status().pool, 1, "the payout waits for a block");
        let _ = fs::remove_dir_all(&folder);
    }
}
