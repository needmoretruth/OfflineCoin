//! A pool on the app's own host, with real node processes doing the hashing over their pipes.
use olc_app::core::{save_coin, Host, Plan, PoolRun, Worker, shares_in};
use olc_app::local::{NodeSpec, Preset, Supervisor, WalletChoice};
use olc_app::settings::Paths;
use olc_core::CoinConfig;
use olc_node::Card;
use olc_wallet::parse_address;
use std::path::PathBuf;
use std::time::{Duration, Instant};

const SHARE_DIFFICULTY: u64 = 2_000;
const BLOCK_DIFFICULTY: u64 = 60_000;
const NODES: u32 = 2;
const WAIT: Duration = Duration::from_secs(120);

fn coin() -> CoinConfig {
    let mut coin = CoinConfig::olc();
    coin.name = "PoolNodes".into();
    coin.ticker = "PNO".into();
    coin.initial_difficulty = BLOCK_DIFFICULTY;
    coin.rules.min_difficulty = 1;
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

fn host(folder: &std::path::Path) -> Host {
    Host::start(
        Plan {
            coin: coin(),
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
fn real_node_processes_earn_real_coins_by_share() {
    let folder = fresh_folder();
    let paths = Paths { root: folder.clone() };
    let host = host(&folder);
    let mut local = Supervisor::with_program(paths.clone(), env!("CARGO_BIN_EXE_olc-core").into());
    let file = save_coin(&paths, &coin()).unwrap();
    let specs: Vec<NodeSpec> = (0..NODES).map(spec).collect();
    for spec in &specs {
        local.start(spec, &file, &[]).unwrap();
    }
    let mut run = PoolRun::new(host.address(), SHARE_DIFFICULTY);
    let started = Instant::now();
    let mut added = 0;
    let mut time = 100_000_000;
    let mut last_height = 0;
    let paid = loop {
        assert!(started.elapsed() < WAIT, "{:?}", run.view());
        if added < NODES {
            added = 0;
            for spec in &specs {
                let status = local.view(spec).status;
                if let Some(address) = status.and_then(|status| parse_address(&status.address)) {
                    run.add_worker(Worker::Node(spec.id), address, spec.name.clone());
                    added += 1;
                }
            }
            continue;
        }
        let mut shares = Vec::new();
        for (id, answer) in local.take_answers() {
            let found = answer.shares.iter().flat_map(|text| shares_in(text));
            shares.extend(found.map(|header| (Worker::Node(id), header)));
        }
        let height = host.view().height;
        if height != last_height {
            last_height = height;
            time += 30_000;
        }
        let lines = {
            let mut runner = host.runner.lock().unwrap();
            run.step(&mut runner.engine, shares, time)
        };
        for (worker, line) in lines {
            if let Worker::Node(id) = worker {
                local.send(id, &line).unwrap();
            }
        }
        let rows = run.view().rows;
        if rows.len() == NODES as usize && rows.iter().all(|row| row.paid > 0) {
            break rows;
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    local.stop_all();
    let total: u64 = paid.iter().map(|row| row.shares).sum();
    assert!(total >= 2, "{paid:?}");
    assert!(run.view().blocks >= 4, "real blocks: {:?}", run.view());
}
