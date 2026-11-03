//! What this build says must still make sense to the build before it. `Older` is that build's
//! list of messages, frozen here, so a change on this side shows up as a failure.
mod common;

use common::{engine_of, mine, silence, test_coin};
use olc_core::coin::Fork;
use olc_core::{Address, Block, CoinConfig, DifficultyRule, Hash, Tx};
use olc_node::{Card, Hello, Keeps, Message, Outgoing};
use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Serialize, Deserialize)]
enum Older {
    Tip {
        genesis: Hash,
        tip: Hash,
        previous: Hash,
        height: u64,
        work: u128,
    },
    GetBlocks(Vec<Hash>),
    Block(Block),
    Tx(Tx),
    Control(Vec<u8>),
}

const SPACED_MS: u64 = 30_000;
const CHAIN: u64 = 900;

#[test]
fn an_older_build_reads_a_request_for_blocks() {
    let ids = vec![[1; 32], [2; 32]];
    let bytes = Message::GetBlocks(ids.clone(), olc_node::Heights(vec![9, 8])).encode();
    assert_eq!(postcard::from_bytes::<Older>(&bytes).unwrap(), Older::GetBlocks(ids));
}

#[test]
fn an_older_build_reads_blocks_and_tips() {
    let (genesis, tip, previous) = ([1; 32], [2; 32], [3; 32]);
    let (height, work) = (5, 6);
    let new = Message::Tip { genesis, tip, previous, height, work }.encode();
    let old = Older::Tip { genesis, tip, previous, height, work };
    assert_eq!(new, postcard::to_allocvec(&old).unwrap());
}

/// A peer far behind, naming only ids, still moves forward from a node that keeps few in memory.
#[test]
fn a_far_behind_older_build_keeps_catching_up() {
    let mut node = engine_of(1, test_coin());
    let mut time = node.chain().tip_header().time_ms;
    let mut ids = vec![node.chain().genesis_id()];
    for _ in 0..CHAIN {
        time += SPACED_MS;
        mine(&mut node, time);
        silence(&mut node);
    }
    let mut got = 0;
    while got < CHAIN {
        time += 1_000;
        node.tick(time);
        let asked = postcard::to_allocvec(&Older::GetBlocks(ids.clone())).unwrap();
        node.on_message(7, Message::decode(&asked).unwrap());
        let mut answered = false;
        while let Some(out) = node.take_outgoing() {
            if let Outgoing::To(7, Message::Block(block)) = out {
                got += 1;
                answered = true;
                assert_eq!(node.chain().id_at(got), Some(block.id()));
                ids.insert(0, block.id());
            }
        }
        assert!(answered, "stuck at {got}");
        ids.truncate(8);
        ids.push(node.chain().genesis_id());
    }
}

/// The coin and hello as builds before floors wrote them.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct OlderRules {
    block_time_ms: u64,
    retarget_interval: u64,
    reward: u64,
    halving_interval: u64,
    coinbase_maturity: u64,
    max_block_bytes: u64,
    min_fee: u64,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct OlderCoin {
    name: String,
    ticker: String,
    message: String,
    genesis_time_ms: u64,
    initial_difficulty: u64,
    premine: Vec<(Address, u64)>,
    rules: OlderRules,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct OlderHello {
    coin: OlderCoin,
    tip: Hash,
    height: u64,
    work: u128,
    address: Address,
    card: Card,
}

fn older_hello(coin: &CoinConfig) -> OlderHello {
    let rules = &coin.rules;
    OlderHello {
        coin: OlderCoin {
            name: coin.name.clone(),
            ticker: coin.ticker.clone(),
            message: coin.message.clone(),
            genesis_time_ms: coin.genesis_time_ms,
            initial_difficulty: coin.initial_difficulty,
            premine: coin.premine.clone(),
            rules: OlderRules {
                block_time_ms: rules.block_time_ms,
                retarget_interval: rules.retarget_interval,
                reward: rules.reward,
                halving_interval: rules.halving_interval,
                coinbase_maturity: rules.coinbase_maturity,
                max_block_bytes: rules.max_block_bytes,
                min_fee: rules.min_fee,
            },
        },
        tip: [7; 32],
        height: 9,
        work: 90,
        address: [8; 20],
        card: Card { name: "board".into(), port: 7000, ..Card::default() },
    }
}

fn hello_of(coin: &CoinConfig) -> Hello {
    let old = older_hello(coin);
    let (tip, height, work, address, card) = (old.tip, old.height, old.work, old.address, old.card);
    Hello { coin: coin.clone(), tip, height, work, address, card }
}

/// The tag this message list gives a hello, which builds before floors share.
fn hello_tag(coin: &CoinConfig) -> u8 {
    Message::Hello(hello_of(coin).into()).encode()[0]
}

#[test]
fn an_older_build_reads_a_hello_with_a_floor() {
    let mut coin = CoinConfig::olc();
    coin.rules.min_difficulty = 12_345;
    let bytes = Message::Hello(hello_of(&coin).into()).encode();
    let read: OlderHello = postcard::from_bytes(&bytes[1..]).unwrap();
    assert_eq!(read, older_hello(&coin));
}

#[test]
fn a_hello_says_what_a_node_keeps_and_older_builds_skip_it() {
    let coin = CoinConfig::olc();
    for keeps in [Keeps::Everything, Keeps::Recent, Keeps::Headers] {
        let mut hello = hello_of(&coin);
        hello.card.keeps = keeps;
        let bytes = Message::Hello(hello.clone().into()).encode();
        assert_eq!(Message::decode(&bytes), Some(Message::Hello(hello.into())));
        let read: OlderHello = postcard::from_bytes(&bytes[1..]).unwrap();
        assert_eq!(read, older_hello(&coin));
    }
    let mut bytes = Message::Hello(hello_of(&coin).into()).encode();
    *bytes.last_mut().unwrap() = 200;
    let Some(Message::Hello(hello)) = Message::decode(&bytes) else {
        panic!("a later kind reads");
    };
    assert_eq!(hello.card.keeps, Keeps::Unsaid);
}

#[test]
fn a_coin_comes_whole_through_a_hello() {
    let mut coin = CoinConfig::olc();
    coin.name = "Newer".into();
    coin.rules.min_difficulty = 12_345;
    coin.message_in_block = true;
    let bytes = Message::Hello(hello_of(&coin).into()).encode();
    assert_eq!(Message::decode(&bytes), Some(Message::Hello(hello_of(&coin).into())));

    // Its difficulty rule, time rules and header, of the base rules and of a fork, come too.
    coin.rules.difficulty_rule = DifficultyRule::Bip94;
    let mut later = coin.rules.clone();
    later.difficulty_rule = DifficultyRule::Asert;
    later.half_life_blocks = 20;
    later.allow_min_difficulty_blocks = false;
    coin.forks.push(Fork { from_height: 50, rules: later });
    let bytes = Message::Hello(hello_of(&coin).into()).encode();
    assert_eq!(Message::decode(&bytes), Some(Message::Hello(hello_of(&coin).into())));
    let read: OlderHello = postcard::from_bytes(&bytes[1..]).unwrap();
    assert_eq!(read, older_hello(&coin), "older builds still read the start");
}

#[test]
fn a_hello_from_an_older_build_settles_its_floor() {
    // An older build knows only coins from before OLC's new genesis: Bitcoin's retarget, a block
    // after its parent, the first header, and no floor unless it said one.
    let mut other = CoinConfig::olc();
    other.name = "Older".into();
    other.bitcoin_header = false;
    other.message_in_block = false;
    other.set_timings(&[olc_core::timing::LEGACY]);
    let mut bytes = vec![hello_tag(&other)];
    bytes.extend(postcard::to_allocvec(&older_hello(&other)).unwrap());
    let Some(Message::Hello(hello)) = Message::decode(&bytes) else {
        panic!("an older hello reads");
    };
    assert_eq!(hello.coin.rules.min_difficulty, 1);
    assert_eq!(hello.genesis(), other.genesis().id());
}
