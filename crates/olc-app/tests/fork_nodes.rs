//! A running coin changes its rules, with real node processes on both sides of the change.
use olc_app::core::{coin_folder, save_coin, Command, Core, CoreLaunch};
use olc_app::local::{ChildStatus, NodeSpec, Preset, Supervisor, WalletChoice};
use olc_app::settings::{Paths, Settings};
use olc_core::coin::COIN;
use olc_core::{Block, Chain, CoinConfig, Plugins};
use olc_pow::ProofOfWork;
use olc_store::open_chain;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

const DIFFICULTY: u64 = 4_000;
const CAP: u64 = 10_000;
const NEW_REWARD: &str = "7";
const WAIT: Duration = Duration::from_secs(120);
const LOOK: Duration = Duration::from_millis(50);
/// Blocks ahead of the tip, so the change reaches every node before the height does.
const AHEAD: u64 = 12;
const STOPPED: Duration = Duration::from_secs(3);

fn coin() -> CoinConfig {
    let mut coin = CoinConfig::olc();
    coin.name = "ForkNodes".into();
    coin.ticker = "FRK".into();
    coin.initial_difficulty = DIFFICULTY;
    coin.rules.min_difficulty = 1;
    coin.rules.floor_from_height = 0;
    coin.rules.retarget_interval = 0;
    coin.rules.block_time_ms = 500;
    coin
}

fn base_id() -> u32 {
    100 + std::process::id() % 600
}

fn spec(id: u32) -> NodeSpec {
    let mut spec = Preset::Esp32S3.spec(id);
    spec.name = format!("miner-{id}");
    spec.hash_cap = CAP;
    spec.memory_mib = 64;
    spec.disk_mib = 256;
    spec.wallet = WalletChoice::Own;
    spec
}

fn fresh_folder() -> PathBuf {
    let folder = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("fork_nodes");
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).unwrap();
    folder
}

fn program() -> PathBuf {
    env!("CARGO_BIN_EXE_olc-core").into()
}

fn core(paths: &Paths, port: u16, local: &NodeSpec) -> Core {
    save_coin(paths, &coin()).unwrap();
    let settings = Settings {
        setup_done: true,
        scan_lan: false,
        broadcast: false,
        port,
        hash_cap: CAP,
        mine_on_start: true,
        coin: Some(coin_folder(&coin())),
        nodes: vec![local.clone()],
        listen_lan: false,
        ..Settings::default()
    };
    let launch = CoreLaunch {
        paths: paths.clone(),
        fleet: None,
        usb: false,
        node_program: Some(program()),
    };
    Core::start(launch, settings).unwrap()
}

fn wait_for(what: &str, mut done: impl FnMut() -> bool) {
    let started = Instant::now();
    while !done() {
        assert!(started.elapsed() < WAIT, "{what}");
        thread::sleep(LOOK);
    }
}

fn height_of(nodes: &mut Supervisor, spec: &NodeSpec) -> u64 {
    nodes.view(spec).status.map_or(0, |status: ChildStatus| status.height)
}

fn pow_plugins() -> Plugins {
    Plugins {
        consensus: Box::new(ProofOfWork),
        languages: olc_lang::all(),
    }
}

/// A chain as a stopped node left it on disk.
fn read_chain(folder: &Path, coin: &CoinConfig) -> Chain {
    open_chain(folder, coin.clone(), pow_plugins(), None).unwrap()
}

fn block_at(chain: &Chain, height: u64) -> Block {
    chain.blocks_from(height, 1).pop().unwrap_or_else(|| panic!("no block at {height}"))
}

fn reward_in(block: &Block) -> u64 {
    block.txs[0].outputs.iter().map(|output| output.value).sum()
}

#[test]
fn a_running_coin_forks_at_a_height_and_a_node_on_the_old_rules_ignores_it() {
    let root = fresh_folder();
    let paths = Paths { root: root.join("app") };
    let local_spec = spec(base_id());
    let port = 21_000 + (std::process::id() % 2_000) as u16;
    let core = core(&paths, port, &local_spec);

    wait_for("the app has no port", || core.snapshot().node.port.is_some());
    let address = format!("127.0.0.1:{}", core.snapshot().node.port.unwrap());
    let old_paths = Paths { root: root.join("old") };
    let old_file = save_coin(&old_paths, &coin()).unwrap();
    let old_spec = spec(base_id() + 1);
    let mut old_node = Supervisor::with_program(old_paths.clone(), program());
    old_node.start(&old_spec, &old_file, &[address]).unwrap();

    wait_for("the nodes never shared a chain", || {
        let snap = core.snapshot();
        snap.node.height >= 5 && height_of(&mut old_node, &old_spec) >= 5
    });

    let from_height = core.snapshot().node.height + AHEAD;
    let changes = vec![("rules.reward".to_string(), NEW_REWARD.to_string())];
    core.send(Command::Fork { at: Some(from_height), changes });
    wait_for("the app never took the fork", || {
        let snap = core.snapshot();
        snap.node.coin.as_ref().is_some_and(|coin| !coin.forks.is_empty())
    });
    let forked = core.snapshot().node.coin.clone().unwrap();
    assert_eq!(forked.genesis().id(), coin().genesis().id(), "same start");
    assert_eq!(forked.rules_at(from_height).reward, 7 * COIN);
    assert_eq!(forked.rules_at(from_height - 1).reward, coin().rules.reward);

    let mut local_height = 0;
    wait_for("the sides did not pass the height", || {
        let snap = core.snapshot();
        local_height = snap.local.first().and_then(|view| view.status.as_ref()).map_or(0, |status| status.height);
        snap.node.height >= from_height + 3
            && local_height >= from_height + 2
            && height_of(&mut old_node, &old_spec) >= from_height + 3
    });

    core.send(Command::ForkBack);
    wait_for("the app did not go back", || {
        let snap = core.snapshot();
        snap.node.coin.as_ref().is_some_and(|coin| coin.forks.is_empty())
    });
    wait_for("the app did not rejoin the old rules", || {
        core.snapshot().node.tip_changes > 0 && core.snapshot().node.height >= from_height + 3
    });
    assert_eq!(core.snapshot().node.coin, Some(coin()));

    drop(core);
    old_node.stop_all();
    thread::sleep(STOPPED);

    let fork_folder = paths.coin(&coin_folder(&forked));
    let fork_chain = read_chain(&fork_folder, &forked);
    let local_chain = read_chain(&paths.local_node(local_spec.id).join(coin_folder(&forked)), &forked);
    let old_chain = read_chain(&old_paths.local_node(old_spec.id).join(coin_folder(&coin())), &coin());

    let last_shared = from_height - 1;
    assert_eq!(block_at(&fork_chain, 1), block_at(&old_chain, 1), "the same first block");
    assert!(fork_chain.height() >= from_height + 2, "history stays: {}", fork_chain.height());
    assert_eq!(block_at(&local_chain, 1), block_at(&fork_chain, 1));
    assert_ne!(block_at(&fork_chain, from_height), block_at(&old_chain, from_height), "they split");
    for chain in [&fork_chain, &local_chain] {
        assert_eq!(reward_in(&block_at(chain, from_height)), 7 * COIN, "the new reward from the height");
        assert_eq!(reward_in(&block_at(chain, last_shared)), coin().rules.reward, "old blocks stay old");
    }
    assert_eq!(block_at(&fork_chain, from_height).header.id(), block_at(&local_chain, from_height).header.id());
    assert_eq!(reward_in(&block_at(&old_chain, from_height)), coin().rules.reward, "the old node ignored it");
    assert_eq!(reward_in(&block_at(&old_chain, from_height + 2)), coin().rules.reward);
}
