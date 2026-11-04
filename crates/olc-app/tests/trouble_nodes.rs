//! The Lab's bug exploiter against a real node process on 127.0.0.1. Every trick is a real
//! consensus violation built on the node's own chain, sent over TCP exactly as a peer would, and
//! refused by the node itself; the refusal is read from the node's status line, never decided on
//! the sender's side. A valid payment and a valid block sent the same way are taken, so the test
//! can tell a real refusal from a broken sender. Before this module existed every assertion here
//! failed: the library, the verdict events and the status field it rides on did not exist.
use olc_app::core::save_coin;
use olc_app::local::trouble::{try_trick, valid_block, valid_payment, Trick};
use olc_app::local::{NodeSpec, Preset, Supervisor, WalletChoice};
use olc_app::settings::Paths;
use olc_core::CoinConfig;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const WAIT: Duration = Duration::from_secs(120);
const LOOK: Duration = Duration::from_millis(100);
/// Enough blocks that a few mining rewards are old enough to spend and a few are still too young.
const MINE_TO: u64 = 14;

fn coin() -> CoinConfig {
    let mut coin = CoinConfig::olc();
    coin.name = "Trouble".into();
    coin.ticker = "TBL".into();
    coin.initial_difficulty = 2_000;
    coin.rules.min_difficulty = 1;
    coin.rules.coinbase_maturity = 4;
    coin.rules.floor_from_height = 0;
    coin.rules.retarget_interval = 0;
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis();
    coin.genesis_time_ms = u64::try_from(now).unwrap() - 5_000;
    coin
}

fn spec(id: u32) -> NodeSpec {
    let mut spec = Preset::Esp32S3.spec(id);
    spec.name = "honest-node".into();
    spec.joined = false;
    spec.mining = true;
    spec.hash_cap = 0;
    spec.wallet = WalletChoice::Own;
    spec
}

fn program() -> PathBuf {
    env!("CARGO_BIN_EXE_olc-core").into()
}

fn wait_for(what: &str, mut done: impl FnMut() -> bool) {
    let started = Instant::now();
    while !done() {
        assert!(started.elapsed() < WAIT, "{what}");
        std::thread::sleep(LOOK);
    }
}

/// Height, tip and balance, read from the node itself, to check a refused trick changed nothing.
fn standing(nodes: &mut Supervisor, spec: &NodeSpec) -> (u64, String, u64) {
    let status = nodes.view(spec).status.expect("the node has said nothing");
    (status.height, status.tip, status.balance)
}

#[test]
fn every_trick_is_refused_by_a_real_node_and_the_valid_ones_are_taken() {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("trouble_nodes");
    let _ = std::fs::remove_dir_all(&root);
    let paths = Paths { root: root.clone() };
    let coin = coin();
    let file = save_coin(&paths, &coin).unwrap();
    let mut nodes = Supervisor::with_program(paths.clone(), program());
    let spec = spec(7);
    let id = spec.id;
    nodes.start(&spec, &file, &[]).unwrap();

    // Mine real blocks so the node holds coins, then stop so its chain stands still.
    wait_for("the node never reached its blocks", || {
        nodes.view(&spec).status.is_some_and(|status| status.height >= MINE_TO)
    });
    nodes.send(id, "mine off").unwrap();
    let addr: std::net::SocketAddr = {
        let port = nodes.view(&spec).status.and_then(|status| status.port).expect("the node has no port");
        format!("127.0.0.1:{port}").parse().unwrap()
    };
    let funds: &Path = &paths.local_node(id).join("wallet");

    // Wait until the node reports it has stopped, then until its height stands still, so no stray
    // mined block lands while a trick is being judged.
    wait_for("the node did not stop mining", || {
        nodes.view(&spec).status.is_some_and(|status| !status.mining)
    });
    let mut last = nodes.view(&spec).status.unwrap().height;
    wait_for("the node kept mining", || {
        std::thread::sleep(Duration::from_millis(400));
        let now = nodes.view(&spec).status.unwrap().height;
        let stable = now == last;
        last = now;
        stable
    });

    // Every trick: refused by the node, with the very rule it should name, and nothing changed.
    for trick in Trick::ALL {
        let before = standing(&mut nodes, &spec);
        let out = {
            let verdicts = || nodes.status(id).map(|status| status.verdicts).unwrap_or_default();
            try_trick(trick, &coin, addr, funds, "honest-node", &verdicts)
        }
        .unwrap_or_else(|error| panic!("{trick:?}: {error}"));
        assert!(out.refused, "{trick:?} was not refused: {out:?}");
        assert!(out.refused_as_expected(), "{trick:?} named the wrong rule: {out:?}");
        assert_eq!(out.by, "honest-node", "{trick:?}");
        // The chain and wallet are untouched by a refused trick.
        wait_for("the node settled after the trick", || {
            std::thread::sleep(Duration::from_millis(200));
            standing(&mut nodes, &spec) == before
        });
    }

    // A valid payment sent the same way is taken into the pool: the sender is not broken.
    let before = standing(&mut nodes, &spec);
    let paid = {
        let verdicts = || nodes.status(id).map(|status| status.verdicts).unwrap_or_default();
        valid_payment(&coin, addr, funds, "honest-node", &verdicts)
    }
    .expect("a valid payment");
    assert!(!paid.refused, "a valid payment was refused: {paid:?}");
    assert_eq!(standing(&mut nodes, &spec), before, "a pooled payment changes no confirmed figure");

    // A valid block sent the same way is taken: the node's chain grows by one.
    let height_before = before.0;
    let built = {
        let verdicts = || nodes.status(id).map(|status| status.verdicts).unwrap_or_default();
        valid_block(&coin, addr, funds, "honest-node", &verdicts)
    }
    .expect("a valid block");
    assert!(!built.refused, "a valid block was refused: {built:?}");
    wait_for("the valid block never joined the chain", || {
        nodes.view(&spec).status.unwrap().height == height_before + 1
    });

    nodes.stop_all();
}
