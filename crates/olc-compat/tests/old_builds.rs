//! Two programs, one set of rules. "Old" is the coin at a fixed published commit.
use old_core as old;
use olc_core::{Block, CoinConfig};
use olc_node::Engine;
use olc_wallet::Wallet;

const DIFFICULTY: u64 = 16;
const BLOCKS: u64 = 300;
const PAY_EVERY: u64 = 5;
const NOTE_EVERY: u64 = 7;

fn plugins() -> olc_core::Plugins {
    olc_core::Plugins {
        consensus: Box::new(olc_pow::ProofOfWork),
        languages: olc_lang::all(),
    }
}

fn old_plugins() -> old::Plugins {
    old::Plugins {
        consensus: Box::new(old_pow::ProofOfWork),
        languages: old_lang::all(),
    }
}

fn coin() -> CoinConfig {
    let mut coin = CoinConfig::olc();
    coin.initial_difficulty = DIFFICULTY;
    coin.rules.coinbase_maturity = 1;
    coin.rules.min_difficulty = 1;
    coin
}

fn old_coin() -> old::CoinConfig {
    let mut coin = old::CoinConfig::olc();
    coin.initial_difficulty = DIFFICULTY;
    coin.rules.coinbase_maturity = 1;
    coin
}

/// A real chain with everything this build added to a block: data outputs and coinbase notes.
fn mined_here() -> Vec<Block> {
    let wallet = Wallet::from_secret([5; 32]).unwrap();
    let mut node = Engine::new(coin(), plugins(), wallet);
    node.set_note(b"a note in the coinbase");
    let friend = Wallet::from_secret([6; 32]).unwrap().address();
    while node.chain().height() < BLOCKS {
        let height = node.chain().height();
        if height > 2 && height.is_multiple_of(PAY_EVERY) {
            // With few ripe coins some of these wait. The chain below shows what got in.
            let _ = node.pay(friend, 1_000, 0);
        }
        if height > 2 && height.is_multiple_of(NOTE_EVERY) {
            let _ = node.write_note(b"a note in its own output");
        }
        let now = node.chain().tip_header().time_ms + node.chain().rules().block_time_ms;
        while !node.mine(100_000, now) {}
        while node.take_outgoing().is_some() {}
    }
    node.chain().blocks_from(1, BLOCKS as usize)
}

#[test]
fn both_builds_start_from_the_same_genesis() {
    assert_eq!(CoinConfig::olc().genesis().id(), old::CoinConfig::olc().genesis().id());
    assert_eq!(coin().genesis().id(), old_coin().genesis().id());
}

#[test]
fn an_old_build_accepts_every_block_made_here_byte_for_byte() {
    let blocks = mined_here();
    let outputs = || blocks.iter().flat_map(|block| &block.txs).flat_map(|tx| &tx.outputs);
    assert!(outputs().filter(|output| output.is_data()).count() as u64 > BLOCKS);
    assert!(blocks.iter().any(|block| block.txs.len() > 1), "real payments too");
    let mut chain = old::Chain::new(old_coin(), old_plugins());
    for block in &blocks {
        let bytes = postcard::to_allocvec(block).unwrap();
        let read: old::Block = postcard::from_bytes(&bytes).unwrap();
        assert_eq!(postcard::to_allocvec(&read).unwrap(), bytes, "one format");
        assert_eq!(read.id(), block.id());
        chain.add_block(read).unwrap();
        assert_eq!(chain.tip(), block.id(), "the old build follows");
    }
    assert_eq!(chain.height(), BLOCKS);
}

#[test]
fn this_build_accepts_every_block_an_old_build_made() {
    let wallet = old_wallet::Wallet::from_secret([5; 32]).unwrap();
    let mut miner = old_node::Engine::new(old_coin(), old_plugins(), wallet);
    let friend = old_wallet::Wallet::from_secret([6; 32]).unwrap().address();
    let mut here = Engine::new(coin(), plugins(), Wallet::from_secret([7; 32]).unwrap());
    let mut time = miner.chain().tip_header().time_ms;
    for height in 0..BLOCKS {
        if height > 2 && height.is_multiple_of(PAY_EVERY) {
            miner.pay(friend, 1_000, 0).unwrap();
        }
        time += 30_000;
        while !miner.mine(100_000, time) {}
        while miner.take_outgoing().is_some() {}
        let block = miner.take_fresh_block().unwrap();
        let bytes = postcard::to_allocvec(&block).unwrap();
        let read: Block = postcard::from_bytes(&bytes).unwrap();
        here.chain_mut().add_block(read).unwrap();
        assert_eq!(here.chain().tip(), miner.chain().tip());
    }
}

#[test]
fn a_coin_crosses_between_builds_in_both_directions() {
    let new = coin();
    let read: old::CoinConfig = postcard::from_bytes(&new.to_bytes()).unwrap();
    assert_eq!(read.genesis().id(), old_coin().genesis().id());
    assert_eq!(read.rules, old_coin().rules);

    let bytes = postcard::to_allocvec(&old::CoinConfig::olc()).unwrap();
    assert_eq!(CoinConfig::from_bytes(&bytes), Some(CoinConfig::olc()));
}
