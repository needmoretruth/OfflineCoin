//! Real rounds: real engines, real hashes, real blocks, and real payments in the chain that hand
//! out every coin a block brought, by proportional and by PPLNS, with and without a fee.
mod common;

use common::{address, node, start_ms, user, Miner};
use olc_node::Engine;
use olc_pool::{pay_owed, BlockState, FoundTotal, Payout, Plan, Pool};

const SHARE_DIFFICULTY: u64 = 32;

/// Mines `blocks` blocks for the pool with two workers, the second sending three shares for each
/// of the first's, then lets the coins mature, pays, and puts the payments in a block.
fn rounds(plan: Plan, blocks: usize) -> (Pool, Engine) {
    let (mut pool, mut pool_node, mut now) = ripe_rounds(plan, blocks);
    assert!(!pool.pay(&mut pool_node).is_empty(), "the ripe blocks are paid");
    now += 30_000;
    while pool_node.status().pool > 0 {
        pool_node.mine(100_000, now);
    }
    (pool, pool_node)
}

/// The rounds of [`rounds`] up to the moment the first blocks' coins can be spent, unpaid.
fn ripe_rounds(plan: Plan, blocks: usize) -> (Pool, Engine, u64) {
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
    (pool, pool_node, now)
}

/// While the node's wallet is locked the pool's payouts wait, say so, and stay owed: kept outside
/// the pool they outlive it, as across a restart, and go out once the wallet opens.
#[test]
fn payouts_wait_for_a_locked_wallet_and_go_out_once_it_opens() {
    let plan = Plan { payout: Payout::Proportional, fee_bps: 0, fixed_difficulty: Some(SHARE_DIFFICULTY) };
    let (mut pool, mut pool_node, mut now) = ripe_rounds(plan, 3);
    let open = pool_node.wallet().clone();
    *pool_node.wallet_mut() = open.locked();
    assert!(pool.pay(&mut pool_node).is_empty(), "a locked wallet signs no payout");
    assert!(pool.waits_for_wallet(), "and the pool says it waits for the wallet");
    let held = pool.blocks().iter().filter(|block| block.held).count();
    assert_eq!(held, 3, "each spendable block says it waits for the wallet");
    let mut owed = pool.owed(&pool_node);
    assert_eq!(owed.len(), 3, "every block found still owes its workers");
    let owed_coins: u64 = owed.iter().flat_map(|debt| &debt.payments).map(|(_, amount)| amount).sum();
    // The app closes: the pool is gone, the payments it owed are not.
    drop(pool);
    assert!(pay_owed(&mut pool_node, &mut owed), "still locked after the restart");
    assert_eq!(owed.len(), 3);
    *pool_node.wallet_mut() = open;
    assert!(!pay_owed(&mut pool_node, &mut owed), "the opened wallet signs");
    assert!(owed.len() < 3, "the ripe payouts went out");
    for _ in 0..4 {
        now += 30_000;
        while !pool_node.mine(100_000, now) {}
        pay_owed(&mut pool_node, &mut owed);
    }
    assert!(owed.is_empty(), "every payout went out as its block ripened");
    while pool_node.status().pool > 0 {
        now += 30_000;
        pool_node.mine(100_000, now);
    }
    let held = pool_node.chain().balance(&address(2)) + pool_node.chain().balance(&address(3));
    assert_eq!(held, owed_coins, "each worker holds what it was owed");
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

/// Two shares on one job may both reach the block's difficulty. The pool sends one block on that
/// parent and counts the other as the work it was, so it never races itself with its own block.
#[test]
fn a_second_share_that_seals_the_same_parent_counts_but_makes_no_second_block() {
    let pool_node = node(1);
    let mut pool = Pool::new(pool_node.address(), Plan { fixed_difficulty: Some(1), ..Plan::default() });
    let now = start_ms();
    pool.refresh(&pool_node, now);
    let mut me = Miner::join(&mut pool, &user(2, "rig"), now);
    let grinder = node(9);
    let mut sealing = 0;
    let mut accepted = 0;
    while sealing < 2 {
        let header = me.find(&grinder);
        assert_eq!(me.submit(&mut pool, &header, now), Ok(()));
        accepted += 1;
        if olc_pool::meets(&header, header.difficulty) {
            sealing += 1;
        }
    }
    assert_eq!(pool.take_blocks().len(), 1, "one block on the parent");
    assert_eq!(pool.blocks().len(), 1);
    assert_eq!(pool.workers(now)[0].accepted, accepted, "every share counted");
}

/// Past the hundred blocks the pool lists, what it found still counts every block, and never
/// falls below what it paid out: the gap is the blocks still ripening.
#[test]
fn the_found_total_outlives_the_blocks_listed() {
    const BLOCKS: u64 = 105;
    let mut pool_node = node(1);
    let own = format!("{}.app", olc_wallet::format_address(&pool_node.address()));
    let mut pool = Pool::new(pool_node.address(), Plan::default());
    let mut now = start_ms();
    assert!(pool.refresh(&pool_node, now));
    let mut me = Miner::join(&mut pool, &own, now);
    let grinder = node(9);
    let (mut found, mut value) = (0, 0);
    while found < BLOCKS {
        let header = me.find(&grinder);
        assert_eq!(me.submit(&mut pool, &header, now), Ok(()));
        let blocks = pool.take_blocks();
        if blocks.is_empty() {
            continue;
        }
        for block in blocks {
            value += block.txs[0].outputs.iter().map(|output| output.value).sum::<u64>();
            let id = block.id();
            let added = pool_node.submit_block(block);
            assert!(added, "the pool's block is valid");
            pool.block_added(id, added);
            found += 1;
        }
        // Each block is paid as it ripens, so the oldest can leave the list.
        assert!(pool.pay(&mut pool_node).is_empty(), "no payment to itself");
        now += 30_000;
        assert!(pool.refresh(&pool_node, now));
        me.listen(&mut pool, now);
    }
    assert_eq!(pool.blocks().len(), 100, "the list stops at a hundred");
    let total = pool.found_total();
    assert_eq!(total, FoundTotal { blocks: BLOCKS, value }, "every block counted");
    assert_eq!(pool.let_go().blocks, BLOCKS - 100);
    let paid = pool.workers(now)[0].paid;
    let ripening: u64 = pool.blocks().iter().filter(|block| block.state != BlockState::Paid).map(|block| block.value).sum();
    assert!(ripening > 0, "the newest blocks still ripen");
    assert_eq!(total.value - paid, ripening, "found and paid differ by the blocks still ripening");
}
