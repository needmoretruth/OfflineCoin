//! Three real nodes of different kinds say what they are over a socket, and the app's list holds
//! it. A running node's hash cap moves from its card without a restart.
use olc_app::core::{coin_folder, save_coin, Command, Core, CoreLaunch};
use olc_app::discovery::Work;
use olc_app::local::{Holding, NodeSpec, Preset, Supervisor, WalletChoice};
use olc_app::settings::{Paths, Settings};
use olc_core::CoinConfig;
use olc_node::Keeps;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

const WAIT: Duration = Duration::from_secs(120);

fn coin() -> CoinConfig {
    let mut coin = CoinConfig::olc();
    coin.name = "KindNodes".into();
    coin.ticker = "KND".into();
    coin.initial_difficulty = 2_000;
    coin.rules.min_difficulty = 1;
    coin.rules.floor_from_height = 0;
    coin.rules.retarget_interval = 0;
    coin.rules.block_time_ms = 500;
    coin
}

fn program() -> PathBuf {
    env!("CARGO_BIN_EXE_olc-core").into()
}

fn wait_for(what: &str, mut done: impl FnMut() -> bool) {
    let started = Instant::now();
    while !done() {
        assert!(started.elapsed() < WAIT, "{what}");
        thread::sleep(Duration::from_millis(50));
    }
}

fn node(id: u32, holding: Holding, mining: bool) -> NodeSpec {
    let wallet = WalletChoice::Own;
    NodeSpec { name: format!("kind-{id}"), holding, mining, wallet, ..Preset::Custom.spec(id) }
}

/// What the app says of the nodes around it, by address.
fn seen(core: &Core) -> BTreeMap<String, (Keeps, bool)> {
    let snap = core.snapshot();
    let chains = snap.groups.iter().flat_map(|group| &group.chains);
    let entries = chains.flat_map(|chain| &chain.entries);
    let rows = entries.filter_map(|entry| {
        let mining = matches!(entry.work, Work::Mining { .. });
        entry.address.clone().map(|address| (address, (entry.keeps, mining)))
    });
    rows.collect()
}

#[test]
fn a_list_of_real_nodes_says_what_each_one_keeps_and_a_cap_moves_on_a_running_node() {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("kind_nodes");
    let _ = std::fs::remove_dir_all(&root);
    let paths = Paths { root: root.join("app") };
    save_coin(&paths, &coin()).unwrap();
    let base = 26_000 + (std::process::id() % 2_000) as u16;
    let mut settings = Settings {
        setup_done: true,
        scan_lan: false,
        broadcast: false,
        port: base,
        coin: Some(coin_folder(&coin())),
        ..Settings::default()
    };
    let own = Preset::Esp32S3.spec(700);
    settings.nodes = vec![own.clone()];
    let launch = CoreLaunch { paths, fleet: None, usb: false, node_program: Some(program()) };
    let core = Core::start(launch, settings).unwrap();
    wait_for("the app has no port", || core.snapshot().node.port.is_some());
    let address = format!("127.0.0.1:{}", core.snapshot().node.port.unwrap());

    let outside = Paths { root: root.join("outside") };
    let file = save_coin(&outside, &coin()).unwrap();
    let mut others = Supervisor::with_program(outside, program());
    let specs = [
        node(701, Holding::Everything, true),
        node(702, Holding::Recent, false),
        node(703, Holding::Headers, false),
    ];
    for spec in &specs {
        others.start(spec, &file, &[address.clone()]).unwrap();
    }
    let mut ports = Vec::new();
    for spec in &specs {
        let mut port = None;
        wait_for("a node has no port", || {
            port = others.view(spec).status.and_then(|status| status.port);
            port.is_some()
        });
        ports.extend(port);
    }
    let said = |core: &Core| {
        let seen = seen(core);
        let of = |port: u16| seen.get(&format!("127.0.0.1:{port}")).copied();
        (of(ports[0]), of(ports[1]), of(ports[2]))
    };
    wait_for("the list never told the three kinds apart", || {
        matches!(
            said(&core),
            (Some((Keeps::Everything, true)), Some((Keeps::Recent, false)), Some((Keeps::Headers, false)))
        )
    });

    let running = || core.snapshot().local.iter().find(|view| view.spec.id == own.id).cloned();
    wait_for("the app's node never mined", || {
        running().and_then(|view| view.status).is_some_and(|status| status.hashrate > 0)
    });
    core.send(Command::NodeCap { id: own.id, cap: 300 });
    wait_for("the running node kept its cap", || {
        running().and_then(|view| view.status).is_some_and(|status| status.cap == 300)
    });
    assert!(running().and_then(|view| view.pid).is_some(), "the node went on running");
    assert_eq!(running().map(|view| view.spec.hash_cap), Some(300), "the card stays with the node");
    others.stop_all();
}
