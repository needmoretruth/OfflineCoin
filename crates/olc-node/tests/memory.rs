//! What a long-lived node keeps in memory, counted by the allocator itself on real mined blocks.
mod common;
mod store;

use common::{mine, silence, test_coin};
use olc_core::Chain;
use olc_node::Engine;
use olc_wallet::{Wallet, MERGE_AT_ONCE};
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
use store::{flash, keep_on, plugins, STATE_SIZE};

struct Counting;

static LIVE: AtomicUsize = AtomicUsize::new(0);

// The one `unsafe` here: `GlobalAlloc` is an unsafe trait, and this only counts and passes on.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        LIVE.fetch_add(layout.size(), Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
        unsafe { System.dealloc(pointer, layout) }
    }
}

#[global_allocator]
static COUNTING: Counting = Counting;

const SPACED_MS: u64 = 30_000;

fn grow(node: &mut Engine, blocks: u64, notes: bool) {
    for _ in 0..blocks {
        let now = node.chain().tip_header().time_ms + SPACED_MS;
        if notes {
            // A payment a block, so the index of recent payments really fills.
            let _ = node.write_note(b"a real payment in a real block");
        }
        mine(node, now);
        for _ in 0..MERGE_AT_ONCE {
            node.tick(now);
        }
        silence(node);
        while node.take_event().is_some() {}
    }
}

fn node_on_flash() -> (Engine, store::Flash) {
    // The bytes of the emulated flash are not what a node holds, so they are all made first.
    let drive = flash(STATE_SIZE * 2 + 6 * 1024 * 1024);
    let chain = Chain::open(test_coin(), plugins(), keep_on(&drive, None));
    (Engine::on_chain(chain, Wallet::from_secret([1; 32]).unwrap()), drive)
}

fn bytes_per_block(node: &mut Engine, blocks: u64, notes: bool) -> f64 {
    let (before, coins) = (LIVE.load(Ordering::Relaxed), node.chain().unspent_count());
    grow(node, blocks, notes);
    let after = LIVE.load(Ordering::Relaxed);
    let per_block = (after as f64 - before as f64) / blocks as f64;
    let (height, now) = (node.chain().height(), node.chain().unspent_count());
    println!("height {height}: {per_block:.2} bytes a block, live {after}, coins {coins} to {now}");
    assert!(now <= coins + 16, "merging keeps a miner's coins few");
    per_block
}

/// The baseline was 484. What is left here is the index of recent payments filling up.
#[test]
fn memory_barely_moves_as_the_chain_grows() {
    let (mut node, _drive) = node_on_flash();
    grow(&mut node, 500, false);
    assert!(bytes_per_block(&mut node, 1_500, false) < 12.0);
}

/// Once that index is full, the only thing still growing is one mark every 256 blocks.
/// Slow without optimisation: `cargo test --release -p olc-node --test memory -- --ignored`
#[test]
#[ignore = "takes minutes without --release"]
fn memory_is_flat_once_the_payment_index_is_full() {
    let (mut node, _drive) = node_on_flash();
    grow(&mut node, 4_400, true);
    // Live bytes wander by about 300 KB as caches turn over, so the span is long.
    assert!(bytes_per_block(&mut node, 24_000, true) < 4.0);
}
