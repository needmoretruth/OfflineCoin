//! A pool on the app's own host: the app's miner first, and real node processes on this computer
//! doing the hashing over their pipes, every one of them a Stratum worker on 127.0.0.1.
use olc_app::core::{save_coin, BlockState, Host, Plan, PoolCx, PoolOrder, PoolSeat, PoolSettings, Who};
use olc_app::local::{NodeSpec, Preset, Supervisor, WalletChoice};
use olc_app::settings::Paths;
use olc_core::CoinConfig;
use olc_node::Card;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const SHARE_DIFFICULTY: u64 = 2_000;
const BLOCK_DIFFICULTY: u64 = 60_000;
const NODES: u32 = 2;
const WAIT: Duration = Duration::from_secs(150);

fn coin() -> CoinConfig {
    let mut coin = CoinConfig::olc();
    coin.name = "PoolNodes".into();
    coin.ticker = "PNO".into();
    coin.initial_difficulty = BLOCK_DIFFICULTY;
    coin.rules.min_difficulty = 1;
    // Born a moment ago, so the difficulty starts where the coin says and not a day behind.
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis();
    coin.genesis_time_ms = u64::try_from(now).unwrap() - 5_000;
    coin
}

fn spec(id: u32) -> NodeSpec {
    let mut spec = Preset::Esp32S3.spec(id);
    spec.name = format!("worker-{id}");
    spec.joined = false;
    spec.mining = false;
    spec.hash_cap = 0;
    spec.wallet = WalletChoice::Own;
    spec
}

fn fresh_folder() -> PathBuf {
    let folder = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("pool_nodes");
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).unwrap();
    folder
}

fn host(folder: &std::path::Path, coin: CoinConfig) -> Host {
    Host::start(
        Plan {
            coin,
            folder: folder.join("chain"),
            wallet_folder: folder.join("wallet"),
            bind_host: "127.0.0.1".into(),
            port: 0,
            port_tries: 1,
            peers: Vec::new(),
            cap: 0,
            mining: false,
            card: Card::default(),
            storage: None,
            shares_wallet: false,
        },
        None,
    )
    .unwrap()
}

#[test]
fn the_apps_miner_and_real_node_processes_are_paid_from_the_pools_blocks() {
    let folder = fresh_folder();
    let paths = Paths { root: folder.clone() };
    let coin = coin();
    let host = host(&folder, coin.clone());
    let mut local = Supervisor::with_program(paths.clone(), env!("CARGO_BIN_EXE_olc-core").into());
    let file = save_coin(&paths, &coin).unwrap();
    let specs: Vec<NodeSpec> = (0..NODES).map(spec).collect();
    for spec in &specs {
        local.start(spec, &file, &[]).unwrap();
    }
    // Only this computer may join: the port the system picks, on 127.0.0.1.
    let mut settings = PoolSettings { port: 0, fixed_difficulty: SHARE_DIFFICULTY, ..PoolSettings::default() };
    assert_eq!(settings.workers, vec![Who::Own], "the app's miner is the first worker");
    let mut seat = PoolSeat::default();
    let started = Instant::now();
    let mut ordered = false;
    let view = loop {
        let mut cx =
            PoolCx { host: &host, local: &mut local, fleet: None, specs: &specs, hellos: &[], settings: &mut settings };
        if !ordered {
            // The nodes' own addresses are known once they answer; they join then.
            let ready = specs.iter().all(|spec| cx.local.view(spec).status.is_some());
            if ready {
                let mut chosen = PoolSettings { port: 0, fixed_difficulty: SHARE_DIFFICULTY, ..PoolSettings::default() };
                chosen.workers.extend(specs.iter().map(|spec| Who::Node(spec.id)));
                assert!(seat.order(PoolOrder::Start(chosen), &mut cx), "{:?}", seat.view().error);
                ordered = true;
            }
        }
        for (id, answer) in cx.local.take_answers() {
            seat.take_node_shares(id, &answer.shares);
        }
        seat.step(&mut cx);
        let view = seat.view();
        assert!(started.elapsed() < WAIT, "{view:#?}");
        let everyone_paid = view.workers.len() == 1 + NODES as usize && view.workers.iter().all(|row| row.paid > 0);
        if everyone_paid {
            break view;
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    assert!(view.running && view.port > 0 && view.reach == vec![format!("127.0.0.1:{}", view.port)]);
    let own = view.workers.iter().find(|row| row.here == Some(Who::Own)).expect("the app's miner works");
    assert!(own.accepted > 0 && own.name.ends_with(".core"), "{own:?}");
    for paid in view.found.iter().filter(|row| row.state == BlockState::Paid) {
        let parts: u64 = paid.parts.iter().map(|(_, amount)| amount).sum();
        assert!(parts + paid.pool_fee <= paid.value, "{paid:?}");
        assert!(paid.value - parts - paid.pool_fee < 1_000, "no more than the payment's fee stays: {paid:?}");
    }
    let mut cx = PoolCx { host: &host, local: &mut local, fleet: None, specs: &specs, hellos: &[], settings: &mut settings };
    assert!(seat.order(PoolOrder::Stop, &mut cx));
    assert!(!seat.view().running);
    local.stop_all();
}
