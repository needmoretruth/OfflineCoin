//! Real rounds: real engines, real hashes, real blocks, and real payments in the chain that hand
//! out every coin a block brought, by proportional and by PPLNS, with and without a fee.
mod common;

use common::{address, node, start_ms, user, Miner};
use olc_node::Engine;
use olc_pool::{BlockState, Payout, Plan, Pool};

const SHARE_DIFFICULTY: u64 = 32;

/// Mines `blocks` blocks for the pool with two workers, the second sending three shares for each
/// of the first's, then lets the coins mature, pays, and puts the payments in a block.
fn rounds(plan: Plan, blocks: usize) -> (Pool, Engine) {
    let mut pool_node = node(1);
    let grinder = node(9);
    let mut pool = Pool::new(pool_node.address(), plan);
    let mut now = start_ms();
    assert!(pool.refresh(&pool_node, now));
    let mut a = Miner::join(&mut pool, &user(2, "rig"), now);
    let mut b = Miner::join(&mut pool, &user(3, "laptop"), now);
    while pool.blocks().len() < blocks {
        for turn in 1.. {
            let miner = if turn % 4 == 0 { &mut a } else { &mut b };
            let header = miner.find(&grinder);
            assert_eq!(miner.submit(&mut pool, &header, now), Ok(()));
            let found = pool.take_blocks();
            if found.is_empty() {
                continue;
            }
            for block in found {
                let id = block.id();
                let added = pool_node.submit_block(block);
                assert!(added, "the pool's block is valid");
                pool.block_added(id, added);
            }
            break;
        }
        now += 30_000;
        assert!(pool.refresh(&pool_node, now));
        a.listen(&mut pool, now);
        b.listen(&mut pool, now);
    }
    // Two more blocks, and the first ones can be spent.
    for _ in 0..3 {
        now += 30_000;
        while !pool_node.mine(100_000, now) {}
    }
    assert!(!pool.pay(&mut pool_node).is_empty(), "the ripe blocks are paid");
    now += 30_000;
    while pool_node.status().pool > 0 {
        pool_node.mine(100_000, now);
    }
    (pool, pool_node)
}

/// Every block's coins add up, and each worker holds in the chain exactly what the pool says it paid.
fn every_coin_lands(pool: &Pool, pool_node: &Engine, fee_bps: u64) {
    let splits = pool.splits();
    assert!(!splits.is_empty());
    for (height, value, split) in &splits {
        let parts: u64 = split.parts.iter().map(|(_, part)| part).sum();
        assert_eq!(parts + split.pool_fee + split.payment_fee, *value, "block {height}");
        assert!(split.pool_fee >= value * fee_bps / 10_000, "block {height}: the fee asked");
    }
    let chain = pool_node.chain();
    let workers = pool.workers(start_ms());
    assert_eq!(workers.len(), 2);
    for worker in &workers {
        assert!(worker.paid > 0, "{} worked and was paid", worker.name);
        assert_eq!(chain.balance(&worker.address), worker.paid, "{} holds what it was paid", worker.name);
    }
    let paid_blocks = pool.blocks().iter().filter(|block| block.state == BlockState::Paid).count();
    assert_eq!(paid_blocks, splits.len());
}

fn paid(pool: &Pool, seed: u8) -> u64 {
    let rows = pool.workers(start_ms());
    rows.iter().find(|row| row.address == address(seed)).map(|row| row.paid).unwrap()
}

#[test]
fn proportional_pays_every_coin_by_work() {
    let plan = Plan { payout: Payout::Proportional, fee_bps: 0, fixed_difficulty: Some(SHARE_DIFFICULTY) };
    let (pool, pool_node) = rounds(plan, 4);
    every_coin_lands(&pool, &pool_node, 0);
    let (a, b) = (paid(&pool, 2), paid(&pool, 3));
    assert!(b > a * 2, "three shares for each one: {a} and {b}");
}

#[test]
fn a_fee_stays_with_the_pool_and_the_rest_is_paid_out() {
    let plan = Plan { payout: Payout::Proportional, fee_bps: 250, fixed_difficulty: Some(SHARE_DIFFICULTY) };
    let (pool, pool_node) = rounds(plan, 3);
    every_coin_lands(&pool, &pool_node, 250);
    let kept: u64 = pool.splits().iter().map(|(_, _, split)| split.pool_fee).sum();
    let values: u64 = pool.splits().iter().map(|(_, value, _)| value).sum();
    assert_eq!(kept, values / 40, "2.5 % of every block");
}

#[test]
fn pplns_pays_every_coin_over_the_last_shares() {
    let plan = Plan { payout: Payout::LastShares { shares: 20 }, fee_bps: 100, fixed_difficulty: Some(SHARE_DIFFICULTY) };
    let (pool, pool_node) = rounds(plan, 4);
    every_coin_lands(&pool, &pool_node, 100);
    let (a, b) = (paid(&pool, 2), paid(&pool, 3));
    assert!(b > a, "more shares in the window, more coins: {a} and {b}");
}

/// A pool of one: the node's own miner is the only worker, and keeps every coin without a payment.
#[test]
fn a_pool_of_one_keeps_its_blocks() {
    let mut pool_node = node(1);
    let own = format!("{}.app", olc_wallet::format_address(&pool_node.address()));
    let mut pool = Pool::new(pool_node.address(), Plan::default());
    let mut now = start_ms();
    pool.refresh(&pool_node, now);
    let mut me = Miner::join(&mut pool, &own, now);
    let grinder = node(9);
    while pool.blocks().is_empty() {
        let header = me.find(&grinder);
        assert_eq!(me.submit(&mut pool, &header, now), Ok(()));
    }
    for block in pool.take_blocks() {
        let id = block.id();
        let added = pool_node.submit_block(block);
        pool.block_added(id, added);
    }
    for _ in 0..3 {
        now += 30_000;
        while !pool_node.mine(100_000, now) {}
    }
    assert!(pool.pay(&mut pool_node).is_empty(), "no payment to itself");
    let (_, value, split) = pool.splits().remove(0);
    assert_eq!(split.parts, vec![(own.clone(), value)]);
    assert_eq!(pool.workers(now)[0].paid, value);
    assert_eq!(pool.blocks()[0].state, BlockState::Paid);
}
