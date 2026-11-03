//! Parsers that work on a chain: a node, a light node and a pool fed what a peer may say, in any
//! order. Each keeps its state from input to input, and starts over now and then.
use crate::fixture::{
    block_at, empty_chain, empty_engine, encoded_messages, grown_engine, messages, plugins,
    quick_coin, wallet, CHAIN_LENGTH,
};
use crate::{Parse, Target};
use olc_core::types::merkle_root;
use olc_core::{CoinConfig, CoinProof, SpendContext};
use olc_node::network::{mark_of, stamp, CONTROL, OPEN};
use olc_node::{Engine, Light, Link, Medium, Message, Runner, Target as Send};
use olc_pool::{Plan, Pool};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

const MOST_HEADERS: usize = 256;
const FRESH_AFTER: u32 = 4_096;
const TICK_EVERY: u32 = 64;
const TICK_MS: u64 = 1_000;

/// The peer a message seems to come from, and whether a damaged block gets a true Merkle root so
/// that it reaches the checks behind that one.
fn peer_of(bytes: &[u8]) -> u32 {
    u32::from(bytes.last().copied().unwrap_or(0) & 7)
}

fn repaired(mut message: Message, bytes: &[u8]) -> Message {
    if let Message::Block(block) = &mut message {
        if !bytes.len().is_multiple_of(4) {
            block.header.merkle_root = merkle_root(&block.txs);
        }
    }
    message
}

/// The first half of the chain the seeds come from, so that a good block can extend it and a
/// damaged one is judged on what it holds.
fn fresh_engine() -> Engine {
    let mut engine = grown_engine(1, CHAIN_LENGTH / 2);
    while engine.take_outgoing().is_some() {}
    engine
}

fn start_engine() -> Parse {
    let mut engine = fresh_engine();
    let mut now = CHAIN_LENGTH * TICK_MS;
    let mut count = 0u32;
    let mut height = engine.chain().height();
    Box::new(move |bytes| {
        if let Some(message) = Message::decode(bytes) {
            crate::taken();
            engine.on_message(peer_of(bytes), repaired(message, bytes));
            if engine.chain().height() > height {
                crate::deep();
                height = engine.chain().height();
            }
        }
        while engine.take_outgoing().is_some() {}
        while engine.take_event().is_some() {}
        count += 1;
        if count.is_multiple_of(TICK_EVERY) {
            now += TICK_MS;
            engine.tick(now);
        }
        if count.is_multiple_of(FRESH_AFTER) {
            engine = fresh_engine();
            height = engine.chain().height();
        }
    })
}

fn start_light() -> Parse {
    let chain = empty_chain();
    let wallet = wallet(3);
    let mut light = Light::new(&chain, MOST_HEADERS);
    let mut held = light.status().held_headers;
    let mut now = 0;
    let mut count = 0u32;
    Box::new(move |bytes| {
        if let Some(message) = Message::decode(bytes) {
            crate::taken();
            light.on_message(peer_of(bytes), message, &chain);
            if light.status().held_headers > held {
                crate::deep();
                held = light.status().held_headers;
            }
        }
        while light.take_outgoing().is_some() {}
        count += 1;
        if count.is_multiple_of(TICK_EVERY) {
            now += TICK_MS;
            light.tick(now, &wallet, &[]);
        }
        if count.is_multiple_of(FRESH_AFTER) {
            light = Light::new(&chain, MOST_HEADERS);
            held = light.status().held_headers;
        }
    })
}

type Heard = VecDeque<(u32, Vec<u8>)>;

#[derive(Clone, Default)]
struct Air(Arc<Mutex<Heard>>);

/// A link with nobody behind it but this program.
struct TestLink {
    heard: Air,
    medium: Medium,
}

impl Link for TestLink {
    fn medium(&self) -> Medium {
        self.medium
    }

    fn send(&mut self, _target: Send, _message: &[u8]) {}

    fn receive(&mut self) -> Option<(u32, Vec<u8>)> {
        self.heard.0.lock().ok()?.pop_front()
    }
}

fn stamped_messages() -> Vec<Vec<u8>> {
    let mark = mark_of(&empty_chain().genesis_id());
    let mut seeds = Vec::new();
    for message in encoded_messages() {
        seeds.push(stamp(mark, &message));
        seeds.push(stamp(OPEN, &message));
        seeds.push(stamp(CONTROL, &message));
    }
    seeds
}

fn start_runner() -> Parse {
    let air = Air::default();
    let start = |air: &Air| {
        let mut runner = Runner::new(fresh_engine());
        for medium in [Medium::EspNow, Medium::Bluetooth, Medium::Wifi, Medium::Wire] {
            runner.add_link(Box::new(TestLink { heard: air.clone(), medium }));
        }
        runner
    };
    let mut runner = start(&air);
    let mut now = 0;
    let mut count = 0u32;
    Box::new(move |bytes| {
        if let Ok(mut heard) = air.0.lock() {
            heard.push_back((peer_of(bytes), bytes.to_vec()));
        }
        now += TICK_MS / 8;
        crate::taken();
        runner.step(now);
        while runner.take_control().is_some() {}
        while runner.take_hello().is_some() {}
        while runner.take_move().is_some() {}
        count += 1;
        if count.is_multiple_of(FRESH_AFTER) {
            runner = start(&air);
        }
    })
}

fn snapshots() -> Vec<Vec<u8>> {
    vec![grown_engine(1, CHAIN_LENGTH).chain().snapshot()]
}

fn start_snapshot() -> Parse {
    let mut chain = empty_chain();
    Box::new(move |bytes| {
        if chain.adopt(bytes, 1).is_ok() {
            crate::deep();
            chain = empty_chain();
        }
    })
}

fn proofs() -> Vec<Vec<u8>> {
    let all = messages();
    let proof = all.iter().filter_map(|message| match message {
        Message::Coin(proof) => postcard::to_allocvec(proof.as_ref()).ok(),
        _ => None,
    });
    proof.collect()
}

fn start_proof() -> Parse {
    let engine = grown_engine(1, CHAIN_LENGTH);
    let header = block_at(engine.chain(), CHAIN_LENGTH).header;
    let address = engine.address();
    Box::new(move |bytes| {
        if let Ok(proof) = postcard::from_bytes::<CoinProof>(bytes) {
            crate::taken();
            if proof.check(&header, &address).is_some() {
                crate::deep();
            }
        }
    })
}

/// A lock, then the unlock, cut where the first two bytes say, in the language the third names.
fn start_script() -> Parse {
    let languages = plugins().languages;
    let context = SpendContext { signature_hash: [4; 32], height: 9, spending: None };
    Box::new(move |bytes| {
        let Some(([first, second, third], rest)) = bytes.split_first_chunk() else {
            return;
        };
        let cut = usize::from(u16::from_le_bytes([*first, *second])).min(rest.len());
        let (lock, unlock) = rest.split_at(cut);
        let language = &languages[usize::from(*third) % languages.len()];
        crate::taken();
        if language.check(lock, unlock, &context) {
            crate::deep();
        }
    })
}

fn scripts() -> Vec<Vec<u8>> {
    let address = [6; 20];
    let lock = olc_lang::beta::pay_to_address(&address);
    let unlock = olc_lang::beta::unlock_address(&[2; 33], &[3; 64]);
    let joined = |language: u8, lock: &[u8], unlock: &[u8]| {
        let cut = (lock.len() as u16).to_le_bytes();
        let head = [cut[0], cut[1], language];
        [&head[..], lock, unlock].concat()
    };
    (0..3).map(|language| joined(language, &lock, &unlock)).collect()
}

fn pool_user() -> String {
    format!("{}.rig", olc_wallet::format_address(&[8; 20]))
}

/// Lines a miner sends a pool: signing in, asking to roll versions, a share.
fn pool_lines() -> Vec<Vec<u8>> {
    let user = pool_user();
    let lines = [
        r#"{"id":1,"method":"mining.subscribe","params":["fuzz"]}"#.to_string(),
        format!(r#"{{"id":2,"method":"mining.authorize","params":["{user}","x"]}}"#),
        r#"{"id":3,"method":"mining.configure","params":[["version-rolling"],{"version-rolling.mask":"1fffe000"}]}"#.into(),
        format!(r#"{{"id":4,"method":"mining.submit","params":["{user}","1","00000001","00000000","00000000"]}}"#),
        format!(r#"{{"id":5,"method":"mining.submit","params":["{user}","1","00000001","00000000","00000000","00002000"]}}"#),
    ];
    lines.into_iter().map(String::into_bytes).collect()
}

/// A pool with one connection signed in, as a miner on the network would find it.
fn open_pool(engine: &Engine) -> (Pool, u64) {
    let mut pool = Pool::new(engine.address(), Plan::default());
    pool.refresh(engine, 1_000);
    let session = pool.open(1_000).unwrap_or_default();
    for line in pool_lines().iter().take(2) {
        pool.handle(session, &String::from_utf8_lossy(line), 1_000);
    }
    (pool, session)
}

fn start_pool() -> Parse {
    let engine = fresh_engine();
    let (mut pool, mut session) = open_pool(&engine);
    let mut count = 0u32;
    Box::new(move |bytes| {
        crate::taken();
        let reply = pool.handle(session, &String::from_utf8_lossy(bytes), 1_000 + u64::from(count));
        if reply.lines.iter().any(|line| line.contains("\"result\":true")) {
            crate::deep();
        }
        count += 1;
        if !reply.keep || count.is_multiple_of(FRESH_AFTER) {
            (pool, session) = open_pool(&engine);
        }
    })
}

fn stranger_coins() -> Vec<Vec<u8>> {
    let mut stranger = quick_coin();
    stranger.name = "Other".into();
    let mut retargets = quick_coin();
    retargets.rules.retarget_interval = 2;
    vec![quick_coin().to_bytes(), stranger.to_bytes(), retargets.to_bytes()]
}

/// A coin taken from a stranger's hello, joined and mined on. Its rules are all outside numbers.
fn run_stranger_coin(bytes: &[u8]) {
    let Some(mut coin) = CoinConfig::from_bytes(bytes) else {
        return;
    };
    coin.settle_floor();
    crate::taken();
    let mut engine = Engine::new(coin, plugins(), wallet(6));
    for turn in 1..4 {
        engine.mine(4, turn * 1_000);
    }
    engine.tick(5_000);
    if engine.chain().height() > 0 {
        crate::deep();
    }
}

/// A node that has only its genesis block, as a new one has on first hearing the world.
fn start_empty_engine() -> Parse {
    let mut engine = empty_engine(4);
    let mut count = 0u32;
    Box::new(move |bytes| {
        if let Some(message) = Message::decode(bytes) {
            crate::taken();
            engine.on_message(peer_of(bytes), repaired(message, bytes));
        }
        while engine.take_outgoing().is_some() {}
        count += 1;
        if count.is_multiple_of(FRESH_AFTER) {
            engine = empty_engine(4);
        }
    })
}

pub fn targets() -> Vec<Target> {
    vec![
        Target { name: "node.engine", seeds: encoded_messages, start: start_engine },
        Target { name: "node.new_engine", seeds: encoded_messages, start: start_empty_engine },
        Target { name: "node.light", seeds: encoded_messages, start: start_light },
        Target { name: "node.runner", seeds: stamped_messages, start: start_runner },
        Target { name: "chain.snapshot", seeds: snapshots, start: start_snapshot },
        Target {
            name: "chain.stranger_coin",
            seeds: stranger_coins,
            start: || Box::new(run_stranger_coin),
        },
        Target { name: "chain.proof", seeds: proofs, start: start_proof },
        Target { name: "chain.script", seeds: scripts, start: start_script },
        Target { name: "pool.line", seeds: pool_lines, start: start_pool },
    ]
}
