//! The minimum difficulty, on real blocks.
mod common;

use common::{engine_of, first_olc, mine, silence, test_coin};
use olc_core::coin::MIN_DIFFICULTY;
use olc_core::{Block, CoinConfig};
use std::path::Path;

const FLOOR: u64 = 16;
const WINDOW: u64 = 4;

fn real_blocks() -> Vec<Block> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/olc-blocks.bin");
    let data = std::fs::read(path).unwrap();
    let mut blocks = Vec::new();
    let mut rest = data.as_slice();
    while let Some((size, after)) = rest.split_first_chunk::<4>() {
        let size = u32::from_le_bytes(*size) as usize;
        blocks.push(postcard::from_bytes(&after[..size]).unwrap());
        rest = &after[size..];
    }
    blocks
}

/// These blocks were mined on boards and computers before there was a floor, and some of them
/// went below it. The floor starts ahead of them, so they all stay valid.
#[test]
fn the_running_chain_stays_valid_under_the_floor() {
    let blocks = real_blocks();
    let lowest = blocks.iter().map(|block| block.header.difficulty).min().unwrap();
    assert!(lowest < MIN_DIFFICULTY, "a chain that needs a starting height");
    let count = blocks.len();
    assert!(first_olc().rules.floor_from_height > count as u64, "the floor starts ahead");

    // The file holds branches that lost too, and every one of them was valid.
    let mut node = engine_of(1, first_olc());
    for block in blocks.clone() {
        node.chain_mut().add_block(block).unwrap();
    }
    let height = node.chain().height();
    assert!(height > 2_000);
    println!("replayed {count} real blocks to height {height}, lowest difficulty {lowest}");

    // A floor from the first block would have thrown part of the real chain away.
    let mut coin = first_olc();
    coin.rules.floor_from_height = 0;
    let mut node = engine_of(1, coin);
    assert!(blocks.into_iter().any(|block| node.chain_mut().add_block(block).is_err()));
}

/// Blocks ten times too slow push the difficulty down until it stops at the floor.
#[test]
fn a_slow_chain_stops_at_the_floor() {
    let mut coin = test_coin();
    coin.initial_difficulty = 256;
    coin.rules.retarget_interval = WINDOW;
    coin.rules.min_difficulty = FLOOR;
    coin.rules.floor_from_height = 0;
    let slow = coin.rules.block_time_ms * 10;
    let mut node = engine_of(1, coin);
    let mut time = node.chain().tip_header().time_ms;
    for _ in 0..WINDOW * 5 {
        time += slow;
        mine(&mut node, time);
        silence(&mut node);
    }
    assert_eq!(node.chain().tip_header().difficulty, FLOOR);
}

/// A chain may sit below a floor that has not started, and climbs to it when it does.
#[test]
fn a_floor_waits_for_its_starting_height() {
    let mut coin = test_coin();
    coin.initial_difficulty = 256;
    coin.rules.retarget_interval = WINDOW;
    coin.rules.min_difficulty = 128;
    let start = WINDOW * 3;
    coin.rules.floor_from_height = start;
    let slow = coin.rules.block_time_ms * 10;
    let mut node = engine_of(1, coin);
    let mut time = node.chain().tip_header().time_ms;
    let mut below = false;
    for _ in 1..start {
        time += slow;
        mine(&mut node, time);
        silence(&mut node);
        below |= node.chain().tip_header().difficulty < 128;
    }
    assert!(below, "nothing stops the fall before the start");
    for _ in 0..WINDOW * 2 {
        time += slow;
        mine(&mut node, time);
        silence(&mut node);
        assert!(node.chain().tip_header().difficulty >= 128);
    }
}

#[test]
fn a_new_coin_starts_where_one_slow_device_can_keep_the_block_time() {
    let rules = CoinConfig::olc().rules;
    assert_eq!(rules.min_difficulty, MIN_DIFFICULTY);
    assert_eq!(MIN_DIFFICULTY, 1_000 * rules.block_time_ms / 1_000);
}

/// Under ASERT, a node whose hash dropped takes the floor the moment its block is late enough,
/// without waiting for its candidate to go stale.
#[test]
fn a_miner_takes_the_floor_once_its_block_is_late() {
    let mut coin = test_coin();
    coin.initial_difficulty = 50_000;
    coin.rules.retarget_interval = WINDOW;
    coin.rules.min_difficulty = FLOOR;
    let rules = coin.rules.clone();
    let mut node = engine_of(1, coin);
    let start = node.chain().tip_header().time_ms;
    mine(&mut node, start + rules.block_time_ms);
    silence(&mut node);
    let first = node.chain().tip_header();
    let late = node.chain().floor_after(&node.chain().tip()).unwrap();
    assert_eq!(late, first.time_ms + 2 * rules.block_time_ms + rules.max_ahead_ms);
    // One try at the regular difficulty, a second before the floor comes.
    assert!(!node.mine(1, late - 1_000));
    assert!(node.mine(1_000, late + 1_000), "two seconds on, the candidate took the floor");
    assert_eq!(node.chain().tip_header().difficulty, FLOOR);
    assert_eq!(node.chain().tip_header().time_ms, late + 1_000);
}
