//! What a miner may send wrong, on purpose or not: each gets Stratum's error and changes nothing.
mod common;

use common::{node, start_ms, user, Miner};
use olc_pool::vardiff::rounded;
use olc_pool::{Plan, Pool};
use serde_json::{json, Value};

const SHARE_DIFFICULTY: u64 = 32;

fn submit(miner: &mut Miner, pool: &mut Pool, params: Value, now: u64) -> Result<(), i64> {
    miner.say(pool, &json!({"id": 50, "method": "mining.submit", "params": params}), now);
    *miner.answers.last().unwrap()
}

#[test]
fn malformed_and_replayed_shares_are_refused_without_harm() {
    let mut pool_node = node(1);
    let grinder = node(9);
    let plan = Plan { fixed_difficulty: Some(SHARE_DIFFICULTY), ..Plan::default() };
    let mut pool = Pool::new(pool_node.address(), plan);
    let now = start_ms();
    pool.refresh(&pool_node, now);
    let name = user(2, "rig");
    let mut miner = Miner::join(&mut pool, &name, now);
    let share = miner.find(&grinder);
    let good = miner.submission(&share)["params"].clone();
    let with = |at: usize, value: &str| {
        let mut params = good.clone();
        params[at] = json!(value);
        params
    };

    // Lines that are not Stratum at all.
    for line in ["", "not json", "{}", "[1,2,3]", "{\"id\":7,\"method\":\"mining.nothing\",\"params\":[]}"] {
        let reply = pool.handle(miner.session, line, now);
        assert!(reply.keep);
        assert!(reply.lines.iter().all(|line| line.contains("\"error\":[")), "{line}: {:?}", reply.lines);
    }
    // Fields of the wrong shape.
    let job = good[1].as_str().unwrap().to_string();
    let wrong_shapes = [
        json!([]),
        json!([name, job]),
        with(2, "zz"),
        with(2, "aabbcc"),
        with(2, "aabbccddee"),
        with(3, "1234"),
        with(3, "123456789"),
        with(4, "nonce!!!"),
        json!([name, job, 5, "00000000", "00000000"]),
    ];
    for params in wrong_shapes {
        assert_eq!(submit(&mut miner, &mut pool, params.clone(), now), Err(20), "{params}");
    }
    // A job that never was, a time the job does not allow, version bits nobody agreed to.
    assert_eq!(submit(&mut miner, &mut pool, with(1, "ffff"), now), Err(21));
    let early = format!("{:08x}", now / 1_000 - 600);
    let late = format!("{:08x}", now / 1_000 + 600);
    assert_eq!(submit(&mut miner, &mut pool, with(3, &early), now), Err(20));
    assert_eq!(submit(&mut miner, &mut pool, with(3, &late), now), Err(20));
    let mut rolled = good.clone();
    rolled.as_array_mut().unwrap().push(json!("ffffffff"));
    assert_eq!(submit(&mut miner, &mut pool, rolled, now), Err(20));
    // A name this connection did not sign in.
    assert_eq!(submit(&mut miner, &mut pool, with(0, &user(3, "thief")), now), Err(24));
    // A hash that does not reach the difficulty.
    let mut weak = share;
    weak.nonce = weak.nonce.wrapping_add(1);
    while olc_pool::meets(&weak, SHARE_DIFFICULTY) {
        weak.nonce = weak.nonce.wrapping_add(1);
    }
    assert_eq!(submit(&mut miner, &mut pool, with(4, &format!("{:08x}", weak.nonce)), now), Err(23));
    assert!(pool.pending().is_empty(), "nothing wrong counted as work");

    // The real share counts once; sent again it is a copy.
    assert_eq!(submit(&mut miner, &mut pool, good.clone(), now), Ok(()));
    assert_eq!(submit(&mut miner, &mut pool, good.clone(), now), Err(22));
    // Another connection sending the same fields hashes its own extranonce: not this share.
    let mut copier = Miner::join(&mut pool, &user(3, "copier"), now);
    let mut copied = good.clone();
    copied[0] = json!(user(3, "copier"));
    assert_eq!(submit(&mut copier, &mut pool, copied, now), Err(23));
    // A connection that never subscribed.
    let silent = pool.open(now).unwrap();
    let reply = pool.handle(silent, &json!({"id": 9, "method": "mining.submit", "params": good}).to_string(), now);
    assert!(reply.lines[0].contains("[25,"), "{:?}", reply.lines);

    let counted: u128 = pool.pending().values().sum();
    assert_eq!(counted, u128::from(rounded(SHARE_DIFFICULTY)), "one share's work, once");
    let rig = pool.workers(now).into_iter().find(|row| row.name == name).unwrap();
    assert_eq!((rig.accepted, rig.refused), (1, 14));
    assert!(pool.take_blocks().is_empty());

    // Once the chain moves, the old job is gone.
    while !pool_node.mine(100_000, now + 30_000) {}
    pool.refresh(&pool_node, now + 30_000);
    let mut stale = good.clone();
    stale[2] = json!("00000099");
    assert_eq!(submit(&mut miner, &mut pool, stale, now + 30_000), Err(21));
}

#[test]
fn a_connection_that_keeps_sending_garbage_is_closed() {
    let pool_node = node(1);
    let mut pool = Pool::new(pool_node.address(), Plan::default());
    let now = start_ms();
    pool.refresh(&pool_node, now);
    let session = pool.open(now).unwrap();
    let kept: Vec<bool> = (0..olc_pool::session::MOST_WRONG).map(|_| pool.handle(session, "garbage", now).keep).collect();
    assert!(kept[..kept.len() - 1].iter().all(|keep| *keep));
    assert!(!kept[kept.len() - 1], "the last one closes it");
}
