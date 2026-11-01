//! A board may run for years, and each flash sector takes 100,000 erases. Counted on the shipped
//! code, on real mined blocks, through reboots, at the block time of a real coin.
mod common;
mod store;

use common::{mine, silence, test_coin};
use olc_core::medium::SECTOR;
use olc_core::Chain;
use olc_node::Engine;
use olc_wallet::Wallet;
use store::{flash, keep_on, plugins, Flash, STATE_SIZE};

/// The datasheet of the board's flash.
const ERASES_A_SECTOR_TAKES: f64 = 100_000.0;
const BLOCK_SECONDS: f64 = 30.0;
const SECONDS_A_YEAR: f64 = 365.25 * 24.0 * 3600.0;
const YEARS_WANTED: f64 = 3.0;
/// Small enough that the log goes round many times in this test.
const LOG_SIZE: usize = 128 * 1024;
/// The block log on a board, from `firmware/src/store.rs`.
const LOG_ON_A_BOARD: f64 = 0x5E_0000 as f64;
const BOOTS: u64 = 6;
const BLOCKS_A_BOOT: u64 = 1_000;

fn grow(drive: &Flash, blocks: u64) {
    let chain = Chain::open(test_coin(), plugins(), keep_on(drive, None));
    let mut node = Engine::on_chain(chain, Wallet::from_secret([1; 32]).unwrap());
    for _ in 0..blocks {
        let now = node.chain().tip_header().time_ms + 30_000;
        let _ = node.write_note(b"a block with something in it");
        mine(&mut node, now);
        node.tick(now);
        silence(&mut node);
        while node.take_event().is_some() {}
    }
}

fn years(erases_at_most: u32, blocks: u64) -> f64 {
    let seconds_a_cycle = blocks as f64 * BLOCK_SECONDS / f64::from(erases_at_most);
    ERASES_A_SECTOR_TAKES * seconds_a_cycle / SECONDS_A_YEAR
}

#[test]
fn no_sector_wears_out_in_three_years() {
    let drive = flash(STATE_SIZE * 2 + LOG_SIZE);
    for _ in 0..BOOTS {
        grow(&drive, BLOCKS_A_BOOT);
    }
    let blocks = BOOTS * BLOCKS_A_BOOT;
    let nor = drive.lock().unwrap();
    let (states, log) = nor.erases.split_at(STATE_SIZE * 2 / SECTOR as usize);
    let state_most = states.iter().copied().max().unwrap();
    let (log_most, log_least) = (*log.iter().max().unwrap(), *log.iter().min().unwrap());
    let state_years = years(state_most, blocks);
    // A log of another size goes round at another pace, and each round erases a sector once.
    let log_years = years(log_most, blocks) * LOG_ON_A_BOARD / LOG_SIZE as f64;
    println!(
        "{blocks} blocks: state sector erased {state_most} times, {state_years:.0} years; log \
         sector {log_least} to {log_most} times, {log_years:.0} years on a board"
    );
    assert!(log_most > 3, "the log must really go round in this test");
    assert!(log_most - log_least <= 1, "the log wears evenly");
    assert!(state_years >= YEARS_WANTED, "{state_years}");
    assert!(log_years >= YEARS_WANTED, "{log_years}");
}
