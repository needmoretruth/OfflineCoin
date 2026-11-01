//! A mining batch must cost the same on a long chain as on a short one. A board runs one batch
//! about every 13 ms: anything in it that walks the chain or the unspent coins turns into a hash
//! rate that falls as the chain grows, and the retarget then hides it as a lower difficulty.
use olc_core::{CoinConfig, Plugins};
use olc_node::Engine;
use olc_pow::ProofOfWork;
use olc_wallet::Wallet;
use std::time::{Duration, Instant};

const SHORT_CHAIN: u64 = 60;
const LONG_CHAIN: u64 = 1_500;
const BATCHES: u32 = 100;
const ROUNDS: u32 = 7;
/// Counting coins on every batch made the long chain about 14 times slower. Tree lookups are
/// meant to cost about twice as much there, so this leaves room for those and a busy machine.
const ALLOWED_GROWTH: f64 = 4.0;
/// A new template is due after this long, so every batch here refreshes one, as a board's does
/// every five seconds.
const STALE_MS: u64 = 6_000;

fn chain_of(blocks: u64) -> Engine {
    let mut config = CoinConfig::olc();
    config.initial_difficulty = 1;
    config.rules.coinbase_maturity = 1;
    config.rules.retarget_interval = 0;
    let plugins = Plugins {
        consensus: Box::new(ProofOfWork),
        languages: olc_lang::all(),
    };
    let mut engine = Engine::new(config, plugins, Wallet::from_secret([3; 32]).unwrap());
    for block in 0..blocks {
        assert!(engine.mine(1_000, block * STALE_MS));
    }
    while engine.take_outgoing().is_some() {}
    while engine.take_event().is_some() {}
    engine
}

/// What a board does per batch apart from hashing, which costs the same at any height.
fn fastest_batches(engine: &mut Engine) -> Duration {
    let mut now = engine.chain().height() * STALE_MS;
    let mut best = Duration::MAX;
    for _ in 0..ROUNDS {
        let started = Instant::now();
        for _ in 0..BATCHES {
            now += STALE_MS;
            engine.tick(now);
            while engine.take_outgoing().is_some() {}
            engine.mine(0, now);
            std::hint::black_box(engine.status());
        }
        best = best.min(started.elapsed());
    }
    best
}

fn assert_little_growth(what: &str, before: Duration, after: Duration) {
    let growth = after.as_secs_f64() / before.as_secs_f64();
    println!("{what}: {before:?} then {after:?}, growth {growth:.2}");
    assert!(
        growth < ALLOWED_GROWTH,
        "{what}: {before:?} became {after:?}"
    );
}

#[test]
fn a_batch_costs_the_same_on_a_long_chain() {
    let short = fastest_batches(&mut chain_of(SHORT_CHAIN));
    let long = fastest_batches(&mut chain_of(LONG_CHAIN));
    assert_little_growth("short chain, long chain", short, long);
}

#[test]
fn a_waiting_payment_adds_nothing_to_a_batch() {
    let mut engine = chain_of(LONG_CHAIN);
    let idle = fastest_batches(&mut engine);
    // Forces the one template that holds the payment, which is the only one that should check it.
    engine
        .pay(Wallet::from_secret([4; 32]).unwrap().address(), 1, 0)
        .unwrap();
    engine.mine(0, LONG_CHAIN * STALE_MS);
    let waiting = fastest_batches(&mut engine);
    assert_little_growth("idle, payment waiting", idle, waiting);
}
