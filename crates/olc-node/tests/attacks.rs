//! What a stranger on a radio or a socket may try, each on real engines and a real chain.
mod common;
mod store;

use common::{deliver, engine, engine_of, mine, silence, test_coin};
use olc_core::{Block, Chain, ChainError, Consensus, Served};
use olc_node::{
    Engine, Event, Heights, Link, Message, Runner, Target, MOST_PEERS, MOST_POOL_BYTES, MOST_QUEUED,
};
use olc_pow::ProofOfWork;
use olc_wallet::{Wallet, WalletError};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use store::{flash, keep_on, plugins, STATE_SIZE};

const NOT_ASKED_ID: [u8; 32] = [0xee; 32];

fn grown(seed: u8, blocks: u64) -> Engine {
    let mut node = engine(seed);
    for height in 1..=blocks {
        mine(&mut node, height * 1_000);
        silence(&mut node);
    }
    node
}

fn events(node: &mut Engine) -> Vec<Event> {
    std::iter::from_fn(|| node.take_event()).collect()
}

/// A wallet that does not know what the node has put aside, so it can spend a coin twice.
fn second_wallet(seed: u8) -> Wallet {
    Wallet::from_secret([seed; 32]).unwrap()
}

/// [a, b, c] and [a, b, c, c] have one Merkle root, and so one block id. The copy with the repeat
/// is invalid. If that verdict stuck to the id, the honest block would be refused for good, and
/// anyone could split a node from the network without a single hash.
#[test]
fn a_block_with_its_last_payment_repeated_says_nothing_about_the_real_one() {
    let mut nodes = [grown(1, 4), engine(2)];
    for node in &mut nodes {
        node.tick(5_000);
    }
    deliver(&mut nodes);
    assert_eq!(nodes[1].status().tip, nodes[0].status().tip);
    let to = second_wallet(3).address();
    let [miner, follower] = &mut nodes;
    miner.pay(to, 1_000, 1).unwrap();
    miner.pay(to, 900, 1).unwrap();
    mine(miner, 6_000);
    silence(miner);
    silence(follower);
    let block = miner.chain().blocks_from(5, 1).pop().unwrap();
    assert_eq!(block.txs.len(), 3, "a block with an odd count of payments");

    let mut twin = block.clone();
    twin.txs.push(twin.txs[2].clone());
    assert_eq!(twin.id(), block.id());
    follower.on_message(0, Message::Block(twin));
    assert_eq!(follower.chain().height(), 4);
    follower.on_message(0, Message::Block(block.clone()));
    assert_eq!(follower.chain().tip(), block.id(), "the real block must still go in");
}

/// Naming a block costs nothing and proves nothing. A claim followed by one worthless block must
/// not get a true block refused, or anyone could cut a node off from the chain everyone is on.
#[test]
fn a_stranger_cannot_get_a_true_block_refused_by_naming_it() {
    let honest = grown(1, 3);
    let (tip, height, work) = (honest.chain().tip(), honest.chain().height(), u128::MAX);
    let genesis = honest.chain().genesis_id();
    let mut node = engine(2);
    node.tick(1_000);
    let claim = Message::Tip { genesis, tip, previous: [1; 32], height, work };
    node.on_message(9, claim);
    silence(&mut node);

    let mut worthless: Block = honest.chain().blocks_from(1, 1).pop().unwrap();
    worthless.header.time_ms = 0;
    while !ProofOfWork.check_seal(&worthless.header) {
        worthless.header.nonce += 1;
    }
    let refused = worthless.id();
    node.on_message(9, Message::Block(worthless));
    assert!(events(&mut node).iter().any(|e| matches!(e, Event::BlockRejected { .. })));

    // The same again, naming the block as the child of the one just refused.
    let claim = Message::Tip { genesis, tip, previous: refused, height, work };
    node.on_message(10, claim);
    silence(&mut node);

    for block in honest.chain().blocks_from(1, 3) {
        node.on_message(0, Message::Block(block));
    }
    assert_eq!(node.chain().tip(), tip, "the true chain goes in");
}

/// Two spends of one coin both pass while neither is in a block, so a person with one coin could
/// fill every node's pool with payments that can never all confirm.
#[test]
fn a_pool_holds_one_spend_of_a_coin() {
    let mut node = grown(1, 4);
    let wallet = second_wallet(1);
    let to = second_wallet(3).address();
    let first = wallet.pay(node.coins(), to, 1_000, 1, &[]).unwrap();
    let again = wallet.pay(node.coins(), to, 2_000, 1, &[]).unwrap();
    assert_ne!(first.id(), again.id());
    assert!(node.submit_tx(first));
    assert!(!node.submit_tx(again), "the first spend seen stays");
    assert_eq!(node.status().pool, 1);
}

/// No block could ever hold it, so it would stay in the pool for good.
#[test]
fn a_payment_too_big_for_any_block_stays_out_of_the_pool() {
    let mut node = grown(1, 4);
    let payments: Vec<_> = (0..300u32).map(|seat| ([seat as u8; 20], 1_000)).collect();
    assert_eq!(node.pay_many(&payments, 1), Err(WalletError::Refused));
    assert_eq!(node.status().pool, 0);
}

/// A board's heap, not a count of payments, is what runs out.
#[test]
fn a_pool_stops_at_its_size_in_bytes() {
    let mut node = grown(1, 80);
    let payments: Vec<_> = (0..100u32).map(|seat| ([seat as u8; 20], 1_000)).collect();
    let mut last_pool = 0;
    let mut most_bytes = 0;
    for _ in 0..70 {
        let wallet = second_wallet(1);
        let reserved: Vec<_> = reserved_by_pool(&node);
        let Ok(tx) = wallet.pay_many(node.coins(), &payments, 1, &reserved) else {
            break;
        };
        most_bytes = most_bytes.max(tx.encode().len());
        node.submit_tx(tx);
        last_pool = node.status().pool;
    }
    assert!(last_pool * most_bytes <= MOST_POOL_BYTES + most_bytes, "{last_pool} x {most_bytes}");
    assert!(last_pool > 8, "the pool should still take a fair share, took {last_pool}");
}

fn reserved_by_pool(node: &Engine) -> Vec<olc_core::OutPoint> {
    node.pool().iter().flat_map(|tx| tx.inputs.iter().map(|input| input.previous)).collect()
}

/// A request of forty bytes answered with eight blocks is a lever. Each name differs, so nothing
/// looks like a repeat.
#[test]
fn a_flood_of_asks_leaves_a_short_queue() {
    let mut node = grown(1, 40);
    let genesis = node.chain().genesis_id();
    for turn in 0u32..2_000 {
        let mut named = [0; 32];
        named[..4].copy_from_slice(&turn.to_le_bytes());
        node.on_message(7, Message::GetBlocks(vec![named, genesis], Heights(vec![40, 0])));
    }
    let queued = std::iter::from_fn(|| node.take_outgoing()).count();
    assert!(queued <= MOST_QUEUED + 8, "{queued} answers queued");
    node.on_message(7, Message::GetCoins { address: node.address(), after: None });
    assert!(node.take_outgoing().is_some(), "an honest ask is answered once the queue is empty");
}

#[test]
fn a_snapshot_may_not_grow_past_its_limit() {
    let mut node = engine(1);
    node.accept_snapshot(5);
    silence(&mut node);
    let part = |part, bytes| Message::SnapshotPart {
        height: 9,
        sum: [7; 32],
        part,
        parts: 3,
        bytes,
    };
    node.on_message(5, part(0, vec![1; 9 * 1024 * 1024]));
    assert!(events(&mut node).iter().any(|e| matches!(e, Event::SnapshotRefused { .. })));
    assert!(node.take_outgoing().is_none(), "no more parts are asked for");
}

/// A made-up address is free, so each one must cost nothing to remember.
#[test]
fn a_flood_of_made_up_peers_stays_on_a_short_list() {
    let mut node = engine(1);
    for peer in 0..10_000 {
        node.on_message(peer, Message::Who);
    }
    assert_eq!(node.status().peers, MOST_PEERS);
}

/// Reading the log costs a board real time, so a block must first show it cost its sender work.
#[test]
fn a_block_with_no_work_reads_nothing_from_the_store() {
    let drive = flash(STATE_SIZE * 2 + 128 * 1024);
    let chain = Chain::open(test_coin(), plugins(), keep_on(&drive, None));
    let mut node = Engine::on_chain(chain, Wallet::from_secret([1; 32]).unwrap());
    node.set_merging(false);
    for height in 1..300 {
        mine(&mut node, height * 1_000);
        silence(&mut node);
    }
    let mut block: Block = node.chain().blocks_from(5, 1).pop().unwrap();
    block.header.previous = NOT_ASKED_ID;
    while ProofOfWork.check_seal(&block.header) {
        block.header.nonce += 1;
    }
    let before = drive.lock().unwrap().reads;
    let refused = node.chain_mut().add_block(block);
    assert_eq!(refused, Err(ChainError::BadSeal));
    assert_eq!(drive.lock().unwrap().reads, before);
}

/// A list without heights can only be placed by reading down the chain.
#[test]
fn asks_without_heights_read_the_chain_once_a_turn() {
    let drive = flash(STATE_SIZE * 2 + 128 * 1024);
    let chain = Chain::open(test_coin(), plugins(), keep_on(&drive, None));
    let mut node = Engine::on_chain(chain, Wallet::from_secret([1; 32]).unwrap());
    node.set_merging(false);
    for height in 1..300 {
        mine(&mut node, height * 1_000);
        silence(&mut node);
    }
    let ask = |turn: u8| Message::GetBlocks(vec![[turn; 32]], Heights(Vec::new()));
    node.tick(400_000);
    silence(&mut node);
    let before = drive.lock().unwrap().reads;
    node.on_message(7, ask(1));
    silence(&mut node);
    let one_walk = drive.lock().unwrap().reads - before;
    assert!(one_walk > 0, "the first ask is answered");
    for turn in 2..100 {
        node.on_message(7, ask(turn));
        silence(&mut node);
    }
    let all = drive.lock().unwrap().reads - before;
    assert_eq!(all, one_walk, "only one ask a turn walks");
}

/// A real locator is short: more than this many names is only work for the one answering.
#[test]
fn only_the_first_names_of_a_locator_count() {
    let node = grown(1, 90);
    let chain = node.chain();
    let real = chain.id_at(5).unwrap();
    let mut locator = vec![(3, NOT_ASKED_ID); 1_000];
    locator.push((5, real));
    let Served::Blocks(blocks) = chain.blocks_after(&locator, 1) else {
        panic!("nothing is pruned here");
    };
    assert_eq!(blocks[0].txs[0].coinbase_height, Some(1));

    let mut names = vec![NOT_ASKED_ID; 1_000];
    names.push(real);
    let Served::Blocks(blocks) = chain.blocks_after_ids(&names, 1) else {
        panic!("nothing is pruned here");
    };
    assert_eq!(blocks[0].txs[0].coinbase_height, Some(1));
}

type Heard = VecDeque<(u32, Vec<u8>)>;

#[derive(Clone, Default)]
struct Queue(Arc<Mutex<Heard>>);

impl Link for Queue {
    fn medium(&self) -> olc_node::Medium {
        olc_node::Medium::EspNow
    }

    fn send(&mut self, _target: Target, _message: &[u8]) {}

    fn receive(&mut self) -> Option<(u32, Vec<u8>)> {
        self.0.lock().unwrap().pop_front()
    }
}

/// A link that always has more must not hold a node inside one turn, or its own mining, its other
/// links and a board's watchdog all wait.
#[test]
fn a_flood_on_one_link_waits_for_its_turn() {
    let queue = Queue::default();
    let flood = olc_node::network::stamp(olc_node::network::OPEN, &Message::Who.encode());
    queue.0.lock().unwrap().extend((0..10_000).map(|_| (1, flood.clone())));
    let mut runner = Runner::new(engine(1));
    runner.add_link(Box::new(queue.clone()));
    runner.step(1_000);
    let left = queue.0.lock().unwrap().len();
    assert!(left >= 9_000, "one turn took {} messages", 10_000 - left);
    for turn in 0..1_000 {
        runner.step(1_000 + turn);
    }
    assert!(queue.0.lock().unwrap().is_empty(), "the rest comes turn by turn");
}

/// Rules arrive with a stranger's coin. Any number there has to work as a rule, or as a coin that
/// never does anything, but not as a crash.
#[test]
fn a_coin_with_the_largest_numbers_only_stands_still() {
    let mut coin = test_coin();
    coin.rules.coinbase_maturity = u64::MAX;
    coin.rules.retarget_interval = u64::MAX;
    coin.rules.block_time_ms = u64::MAX;
    let mut node = engine_of(1, coin);
    for height in 1..4 {
        mine(&mut node, height * 1_000);
    }
    assert_eq!(node.chain().height(), 3);
    assert_eq!(node.chain().balance(&node.address()), node.status().balance);
    assert!(node.pay(node.address(), 1, 1).is_err(), "nothing ever matures");
}
