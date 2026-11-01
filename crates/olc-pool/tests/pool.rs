//! A real round: real engines, real hashes, real blocks and a real payment in the chain.
use olc_core::{Address, Chain, CoinConfig, Plugins};
use olc_node::{Engine, Message, Outgoing};
use olc_pool::{Job, Pool, Verdict};
use olc_pow::ProofOfWork;
use olc_wallet::Wallet;

const SHARE_DIFFICULTY: u64 = 32;
const BLOCK_DIFFICULTY: u64 = 800;

fn coin() -> CoinConfig {
    let mut coin = CoinConfig::olc();
    coin.name = "PoolProof".into();
    coin.ticker = "POP".into();
    coin.initial_difficulty = BLOCK_DIFFICULTY;
    coin.rules.min_difficulty = 1;
    coin
}

fn node(seed: u8) -> Engine {
    let plugins = Plugins {
        consensus: Box::new(ProofOfWork),
        languages: olc_lang::all(),
    };
    let chain = Chain::new(coin(), plugins);
    let mut engine = Engine::on_chain(chain, Wallet::from_secret([seed; 32]).unwrap());
    engine.set_merging(false);
    engine
}

fn address(seed: u8) -> Address {
    Wallet::from_secret([seed; 32]).unwrap().address()
}

/// Passes on everything `from` wants to say, the way a link would.
fn relay(from: &mut Engine, to: &mut Engine) {
    while let Some(outgoing) = from.take_outgoing() {
        let (Outgoing::All { message, .. } | Outgoing::To(_, message)) = outgoing;
        if matches!(message, Message::Block(_) | Message::Tx(_)) {
            to.on_message(0, message);
        }
    }
    while to.take_outgoing().is_some() {}
}

/// Grinds as the worker's own node does and returns the first share.
fn share(worker: &Engine, job: &mut Job) -> olc_core::BlockHeader {
    while !worker.grind(&mut job.header, job.difficulty, 1_000) {}
    let found = job.header;
    job.header.nonce = job.header.nonce.wrapping_add(1);
    found
}

#[test]
fn a_round_pays_by_share_with_an_ordinary_transaction() {
    let mut pool_node = node(1);
    let mut worker_a = node(2);
    let worker_b = node(3);
    let mut pool = Pool::new(pool_node.address(), SHARE_DIFFICULTY);
    let (a, b) = (address(2), address(3));
    let mut time = 100_000;
    let mut shares = 0;
    while pool.blocks_found() < 5 {
        pool.is_out_of_date(&pool_node);
        time += 30_000;
        let mut job_a = pool.job_for(&pool_node, a, time);
        let mut job_b = pool.job_for(&pool_node, b, time);
        // The second worker hashes three times for each of the first's.
        for turn in 1.. {
            let (worker, job, who) = match turn % 4 {
                0 => (&worker_a, &mut job_a, a),
                _ => (&worker_b, &mut job_b, b),
            };
            let header = share(worker, job);
            shares += 1;
            match pool.submit(&mut pool_node, who, &header) {
                Verdict::Share => {}
                Verdict::Block { .. } => break,
                other => panic!("{other:?}"),
            }
        }
    }
    let paid = pool.pay(&mut pool_node);
    assert!(!paid.is_empty(), "the first rounds are ripe");
    while pool_node.status().pool > 0 {
        pool_node.mine(100_000, time + 30_000);
    }
    relay(&mut pool_node, &mut worker_a);
    let chain = pool_node.chain();
    let tally: Vec<_> = pool.tally().map(|(who, tally)| (*who, *tally)).collect();
    assert_eq!(tally.iter().map(|(_, t)| t.shares).sum::<u64>(), shares);
    for (who, tally) in tally {
        assert!(tally.paid > 0);
        assert_eq!(chain.balance(&who), tally.paid, "paid in real coins");
    }
    let (rate_a, rate_b) = (pool_tally(&pool, &a).paid, pool_tally(&pool, &b).paid);
    assert!(rate_b > rate_a, "more shares, more coins: {rate_a} and {rate_b}");
}

fn pool_tally(pool: &Pool, who: &Address) -> olc_pool::Tally {
    pool.tally().find(|(known, _)| *known == who).map(|(_, tally)| *tally).unwrap()
}

#[test]
fn only_real_work_counts() {
    let mut pool_node = node(1);
    let worker = node(2);
    let mut pool = Pool::new(pool_node.address(), SHARE_DIFFICULTY);
    let (a, b) = (address(2), address(3));
    assert!(pool.is_out_of_date(&pool_node));
    let mut job = pool.job_for(&pool_node, a, 100_000);

    let header = share(&worker, &mut job);
    assert_eq!(pool.submit(&mut pool_node, a, &header), Verdict::Share);
    assert_eq!(pool.submit(&mut pool_node, a, &header), Verdict::Duplicate);
    // Someone else's share is not yours.
    assert_eq!(pool.submit(&mut pool_node, b, &header), Verdict::Forged);

    let mut weak = header;
    while pool_node.work_is_done(&weak, SHARE_DIFFICULTY) {
        weak.nonce = weak.nonce.wrapping_add(1);
    }
    assert_eq!(pool.submit(&mut pool_node, a, &weak), Verdict::Weak);

    let mut changed = header;
    changed.difficulty = 1;
    assert_eq!(pool.submit(&mut pool_node, a, &changed), Verdict::Forged);

    let mut unknown = header;
    unknown.merkle_root = [9; 32];
    assert_eq!(pool.submit(&mut pool_node, a, &unknown), Verdict::Stale);

    // Once the chain moves, the job is worthless.
    while !pool_node.mine(100_000, 130_000) {}
    assert!(pool.is_out_of_date(&pool_node));
    let later = share(&worker, &mut job);
    assert_eq!(pool.submit(&mut pool_node, a, &later), Verdict::Stale);
    assert_eq!(pool_tally(&pool, &a).shares, 1);
    assert_eq!(pool_tally(&pool, &a).rejected, 5);
}
