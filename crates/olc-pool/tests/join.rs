//! Two nodes on this computer's loopback: one runs the pool and mines in it with its own miner,
//! the other joins by `host:port` and `address.worker`. Both send shares over TCP, the pool finds
//! blocks, and once they can be spent each node holds in its chain what the pool paid it.
mod common;

use common::{catch_up, coin_now, node_on, user};
use olc_pool::{unix_ms, Client, Link, Plan, Pool, Server};
use olc_wallet::format_address;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

/// A miner's loop: work from the client, hashed with OLC's own proof of work, shares sent back.
fn mine(client: Arc<Client>, coin: olc_core::CoinConfig, stop: Arc<AtomicBool>) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let grinder = node_on(coin, 9);
        let mut extranonce2 = 0;
        while !stop.load(Ordering::Relaxed) {
            let generation = client.generation();
            extranonce2 += 1;
            let Some(work) = client.work(extranonce2) else {
                thread::sleep(Duration::from_millis(20));
                continue;
            };
            let mut header = work.header;
            for _ in 0..50 {
                if stop.load(Ordering::Relaxed) || client.generation() != generation {
                    break;
                }
                if grinder.grind(&mut header, work.difficulty, 2_000) {
                    client.submit(&header);
                    header.nonce = header.nonce.wrapping_add(1);
                    // As slow as a small miner: a few shares a second at most.
                    thread::sleep(Duration::from_millis(50));
                }
            }
        }
    })
}

#[test]
fn a_node_that_joins_by_address_and_name_is_paid_from_the_pools_block() {
    let coin = coin_now();
    let mut pool_node = node_on(coin.clone(), 1);
    let mut joiner = node_on(coin.clone(), 2);
    let pool = Arc::new(Mutex::new(Pool::new(pool_node.address(), Plan::default())));
    pool.lock().unwrap().refresh(&pool_node, unix_ms());
    let server = Server::start(SocketAddr::from(([127, 0, 0, 1], 0)), pool.clone()).unwrap();
    let at = format!("127.0.0.1:{}", server.address().port());

    // The pool's own miner is its first worker; the other node joins from "another computer".
    let own = format!("{}.app", format_address(&pool_node.address()));
    let mine_here = Arc::new(Client::start(&at, &own, "x"));
    let mine_there = Arc::new(Client::start(&at, &user(2, "laptop"), "x"));
    let stop = Arc::new(AtomicBool::new(false));
    let miners = [mine(mine_here.clone(), coin.clone(), stop.clone()), mine(mine_there.clone(), coin.clone(), stop.clone())];

    let started = Instant::now();
    loop {
        assert!(started.elapsed() < Duration::from_secs(120), "the pool found its blocks in time");
        thread::sleep(Duration::from_millis(100));
        let mut pool = pool.lock().unwrap();
        for block in pool.take_blocks() {
            let id = block.id();
            let added = pool_node.submit_block(block);
            pool.block_added(id, added);
        }
        pool.refresh(&pool_node, unix_ms());
        let both = pool.workers(unix_ms()).iter().filter(|row| row.accepted > 0).count() == 2;
        if both && pool.blocks().len() >= 2 {
            break;
        }
    }
    stop.store(true, Ordering::Relaxed);
    for miner in miners {
        miner.join().unwrap();
    }
    // The last answers come in, then both miners leave.
    thread::sleep(Duration::from_secs(1));
    let (here, there) = (mine_here.view(), mine_there.view());
    drop((mine_here, mine_there));
    assert_eq!(here.link, Link::Working);
    assert_eq!(there.link, Link::Working);
    assert!(here.accepted > 0 && there.accepted > 0, "{here:?} {there:?}");
    // Each worker knows whose blocks it helps find: the address the pool's coinbase pays.
    assert_eq!(there.payee, Some(pool_node.address()), "the joiner names the pool by its payout address");

    // The coins mature, the pool pays, and the payment reaches the other node in a block.
    let mut held = pool.lock().unwrap();
    let pool = &mut *held;
    for _ in 0..3 {
        while !pool_node.mine(100_000, unix_ms()) {}
    }
    assert!(!pool.pay(&mut pool_node).is_empty());
    while pool_node.status().pool > 0 {
        pool_node.mine(100_000, unix_ms());
    }
    catch_up(&pool_node, &mut joiner);
    let rows = pool.workers(unix_ms());
    let laptop = rows.iter().find(|row| row.name == user(2, "laptop")).unwrap();
    let app = rows.iter().find(|row| row.name == own).unwrap();
    assert!(laptop.paid > 0 && app.paid > 0, "both paid: {} and {}", laptop.paid, app.paid);
    assert_eq!(joiner.chain().height(), pool_node.chain().height(), "the other node follows");
    assert_eq!(joiner.chain().balance(&laptop.address), laptop.paid, "in the other node's own chain");
    assert_eq!(laptop.accepted, there.accepted, "every share the pool took, the joiner saw taken");
    // The port closes after the pool is let go: its connections take the pool's lock to leave.
    drop(held);
    drop(server);
}

/// A share found on a job the pool ended with a new block is kept back, as Bitcoin's miners keep
/// it, instead of being sent only to be refused.
#[test]
fn a_share_of_a_job_a_new_block_ended_is_kept_back() {
    let coin = coin_now();
    let mut pool_node = node_on(coin.clone(), 1);
    let pool = Arc::new(Mutex::new(Pool::new(pool_node.address(), Plan { fixed_difficulty: Some(1), ..Plan::default() })));
    pool.lock().unwrap().refresh(&pool_node, unix_ms());
    let server = Server::start(SocketAddr::from(([127, 0, 0, 1], 0)), pool.clone()).unwrap();
    let client = Client::start(&format!("127.0.0.1:{}", server.address().port()), &user(2, "rig"), "x");
    let grinder = node_on(coin, 9);
    let wait = |done: &dyn Fn() -> bool| {
        let started = Instant::now();
        while !done() {
            assert!(started.elapsed() < Duration::from_secs(10), "waited too long");
            thread::sleep(Duration::from_millis(10));
        }
    };
    wait(&|| client.work(1).is_some());
    let work = client.work(2).unwrap();
    let mut old = work.header;
    while !grinder.grind(&mut old, work.difficulty, 1_000) {}

    // A block comes from elsewhere: the pool's next job starts on it and ends every older one.
    let generation = client.generation();
    while !pool_node.mine(100_000, unix_ms()) {}
    pool.lock().unwrap().refresh(&pool_node, unix_ms());
    wait(&|| client.generation() != generation);

    client.submit(&old);
    thread::sleep(Duration::from_millis(300));
    let view = client.view();
    assert_eq!((view.stale, view.refused), (1, 0), "kept back, not refused: {view:?}");
    drop(client);
    drop(server);
}
