//! A small real chain to take good inputs from, and to run the stateful parsers on. Difficulty 1
//! lets any header pass the seal, so damaged blocks reach the checks behind it.
use olc_core::proof::CoinProof;
use olc_core::{Block, BlockHeader, Chain, CoinConfig, OutPoint, Plugins};
use olc_node::{Card, Engine, Heights, Hello, Message, Outgoing};
use olc_pow::ProofOfWork;
use olc_wallet::Wallet;

const STEP_MS: u64 = 30_000;
const PAY_EVERY: u64 = 3;
pub const CHAIN_LENGTH: u64 = 24;

pub fn quick_coin() -> CoinConfig {
    let mut coin = CoinConfig::olc();
    coin.initial_difficulty = 1;
    coin.rules.min_difficulty = 1;
    coin.rules.coinbase_maturity = 1;
    coin.rules.retarget_interval = 0;
    coin
}

pub fn plugins() -> Plugins {
    Plugins { consensus: Box::new(ProofOfWork), languages: olc_lang::all() }
}

pub fn empty_chain() -> Chain {
    Chain::new(quick_coin(), plugins())
}

pub fn wallet(seed: u8) -> Wallet {
    Wallet::from_secret([seed; 32]).expect("a fixed secret is a good one")
}

pub fn empty_engine(seed: u8) -> Engine {
    let mut engine = Engine::new(quick_coin(), plugins(), wallet(seed));
    engine.set_merging(false);
    engine
}

fn drain(engine: &mut Engine) {
    while engine.take_outgoing().is_some() {}
    while engine.take_event().is_some() {}
}

/// An engine that mined a short chain of its own, with a payment in every few blocks. The same
/// seed gives the same chain, so a shorter one can be fed the later blocks of a longer one.
pub fn grown_engine(seed: u8, length: u64) -> Engine {
    let mut engine = empty_engine(seed);
    let to = wallet(seed.wrapping_add(1)).address();
    for height in 1..=length {
        if height.is_multiple_of(PAY_EVERY) {
            let _ = engine.pay(to, 1_000 + height, 10);
        }
        while !engine.mine(16, height * STEP_MS) {}
        drain(&mut engine);
    }
    engine
}

pub fn block_at(chain: &Chain, height: u64) -> Block {
    chain.blocks_from(height, 1).pop().expect("the fixture holds its chain")
}

/// One message of every kind, built on a real chain.
pub fn messages() -> Vec<Message> {
    let mut engine = grown_engine(1, CHAIN_LENGTH);
    let chain = engine.chain();
    let genesis = chain.genesis_id();
    let block = block_at(chain, CHAIN_LENGTH);
    let every_block = (1..=CHAIN_LENGTH).map(|height| Message::Block(block_at(chain, height)));
    let every_block: Vec<Message> = every_block.collect();
    let tip = block.id();
    let previous = block.header.previous;
    let address = engine.address();
    let point = OutPoint { txid: block.txs[0].id(), index: 0 };
    let proof = CoinProof::of(&block, CHAIN_LENGTH, &point).expect("a proof of a coinbase");
    let snapshot = chain.snapshot();
    let headers: Vec<BlockHeader> =
        (1..4).map(|height| block_at(chain, height).header).collect();
    let hello = Hello {
        coin: quick_coin(),
        tip,
        height: CHAIN_LENGTH,
        work: 40,
        address,
        card: Card { name: "seed".into(), device: "computer".into(), ..Card::default() },
    };
    let mut all = vec![
        Message::Tip { genesis, tip, previous, height: CHAIN_LENGTH, work: 40 },
        Message::GetBlocks(vec![tip, genesis], Heights(vec![CHAIN_LENGTH, 0])),
        Message::Block(block),
        Message::Control(vec![1, 2, 3]),
        Message::Who,
        Message::Hello(Box::new(hello)),
        Message::Pruned { oldest: 3 },
        Message::GetSnapshot { part: 0 },
        Message::SnapshotPart {
            height: CHAIN_LENGTH,
            sum: tip,
            part: 0,
            parts: 1,
            bytes: snapshot,
        },
        Message::GetHeaders(vec![(CHAIN_LENGTH, tip), (0, genesis)]),
        Message::Headers(headers),
        Message::GetCoins { address, after: None },
        Message::GetCoins { address, after: Some(point) },
        Message::Coin(Box::new(proof)),
        Message::CoinsEnd { address, last: Some(point), more: true },
        Message::Here(7),
        // A channel packet rides the link to the desktop's channel node; boards pass it by.
        Message::Channel(b"OLC channel envelope".to_vec()),
        Message::Channel(Vec::new()),
    ];
    let _ = engine.pay(address, 1_000, 10);
    while let Some(outgoing) = engine.take_outgoing() {
        if let Outgoing::All { message: message @ Message::Tx(_), .. } = outgoing {
            all.push(message);
        }
    }
    assert!(all.iter().any(|message| matches!(message, Message::Tx(_))), "no payment to damage");
    all.extend(every_block);
    all
}

pub fn encoded_messages() -> Vec<Vec<u8>> {
    messages().iter().map(Message::encode).collect()
}
