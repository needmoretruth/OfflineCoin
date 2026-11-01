//! Mines a long real chain on this computer, for measuring and for feeding to boards. Every block
//! has real work at a low difficulty and real signed payments. Only its clock is hurried: blocks
//! are stamped one target time apart, so the difficulty holds still.
//!
//! `grow_chain FOLDER BLOCKS [DIFFICULTY]`
use olc_core::{Chain, CoinConfig, Plugins};
use olc_node::Engine;
use olc_pow::ProofOfWork;
use olc_store::coin_file::to_json;
use olc_store::{load_wallet, open_chain, save_wallet};
use olc_wallet::Wallet;
use std::path::Path;
use std::time::Instant;

const PAY_EVERY: u64 = 7;
const SHOW_EVERY: u64 = 5_000;

fn plugins() -> Plugins {
    Plugins {
        consensus: Box::new(ProofOfWork),
        languages: olc_lang::all(),
    }
}

fn coin(difficulty: u64) -> CoinConfig {
    let mut coin = CoinConfig::olc();
    coin.name = "LongChain".into();
    coin.ticker = "LNG".into();
    coin.message = "A long real chain for measuring".into();
    coin.initial_difficulty = difficulty;
    coin.message_in_block = true;
    coin
}

fn resident_kib() -> u64 {
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    let line = status.lines().find(|line| line.starts_with("VmRSS:"));
    let number = line.and_then(|line| line.split_whitespace().nth(1));
    number.and_then(|text| text.parse().ok()).unwrap_or(0)
}

fn open(folder: &Path, coin: CoinConfig) -> Chain {
    open_chain(&folder.join("chain"), coin, plugins(), None).expect("the chain opens")
}

fn main() {
    let words: Vec<String> = std::env::args().skip(1).collect();
    let folder = Path::new(&words[0]);
    let blocks: u64 = words[1].parse().expect("a number of blocks");
    let difficulty = words.get(2).map_or(16, |text| text.parse().expect("a difficulty"));
    let coin = coin(difficulty);
    std::fs::create_dir_all(folder).expect("the folder");
    std::fs::write(folder.join("coin.json"), to_json(&coin).expect("the coin")).expect("the coin");
    let wallet = load_wallet(folder).expect("the wallet");
    let other = Wallet::from_secret([7; 32]).expect("a valid key").address();
    save_wallet(folder, &wallet).expect("the wallet");

    let started = Instant::now();
    let chain = open(folder, coin.clone());
    let rules = chain.rules().clone();
    let mut node = Engine::on_chain(chain, wallet);
    let mut paid = 0u64;
    while node.chain().height() < blocks {
        let height = node.chain().height();
        let now = node.chain().tip_header().time_ms + rules.block_time_ms;
        if height % PAY_EVERY == 0 && height > rules.coinbase_maturity + 1 {
            paid += u64::from(node.pay(other, 1_000, rules.min_fee).is_ok());
        }
        while !node.mine(100_000, now) {}
        node.tick(now);
        while node.take_outgoing().is_some() {}
        while node.take_event().is_some() {}
        if height % SHOW_EVERY == 0 {
            let held = node.chain().held();
            println!(
                "height={height} seconds={} rss_kib={} unspent={} bytes={} paid={paid}",
                started.elapsed().as_secs(),
                resident_kib(),
                node.chain().unspent_count(),
                held.bytes,
            );
        }
    }
    let held = node.chain().held();
    println!(
        "done height={} bytes={} per_block={} seconds={} unspent={} rss_kib={}",
        node.chain().height(),
        held.bytes,
        held.bytes / node.chain().height().max(1),
        started.elapsed().as_secs(),
        node.chain().unspent_count(),
        resident_kib(),
    );
    node.chain_mut().save_now();
    drop(node);

    let opening = Instant::now();
    let chain = open(folder, coin);
    println!(
        "reopened height={} ms={} opened={:?} rss_kib={}",
        chain.height(),
        opening.elapsed().as_millis(),
        chain.opened(),
        resident_kib(),
    );
}
