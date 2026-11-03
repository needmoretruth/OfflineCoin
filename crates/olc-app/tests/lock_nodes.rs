//! Coins under beta locks move between two real nodes, one as the app runs it and one as a process
//! with a board's memory limit, and a wrong unlock moves nothing on either.
use olc_app::core::host_locks::{hash_lock_for, two_of_two_for};
use olc_app::core::{coin_folder, save_coin, Command, Core, CoreLaunch, LockOrder};
use olc_app::local::{ChildStatus, NodeSpec, Preset, Supervisor, WalletChoice};
use olc_app::settings::{Paths, Settings};
use olc_core::CoinConfig;
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

const WAIT: Duration = Duration::from_secs(120);
const LOOK: Duration = Duration::from_millis(50);
const SECRET: &str = "a long way home";

fn coin() -> CoinConfig {
    let mut coin = CoinConfig::olc();
    coin.name = "LockNodes".into();
    coin.ticker = "LCK".into();
    coin.initial_difficulty = 2_000;
    coin.rules.min_difficulty = 1;
    coin.rules.floor_from_height = 0;
    coin.rules.retarget_interval = 0;
    coin.rules.block_time_ms = 300;
    coin.rules.coinbase_maturity = 1;
    coin
}

fn program() -> PathBuf {
    env!("CARGO_BIN_EXE_olc-core").into()
}

fn wait_for(what: &str, mut done: impl FnMut() -> bool) {
    let started = Instant::now();
    while !done() {
        assert!(started.elapsed() < WAIT, "{what}");
        thread::sleep(LOOK);
    }
}

fn app(paths: &Paths, port: u16) -> Core {
    save_coin(paths, &coin()).unwrap();
    let settings = Settings {
        setup_done: true,
        scan_lan: false,
        broadcast: false,
        port,
        hash_cap: 10_000,
        mine_on_start: true,
        coin: Some(coin_folder(&coin())),
        listen_lan: false,
        ..Settings::default()
    };
    let launch =
        CoreLaunch { paths: paths.clone(), fleet: None, usb: false, node_program: Some(program()) };
    Core::start(launch, settings).unwrap()
}

/// What the app's core said to an order.
fn told(core: &Core, order: LockOrder) -> (bool, String) {
    let before = core.snapshot().notice.as_ref().map_or(0, |notice| notice.number);
    core.send(Command::Lock(order));
    let mut said = None;
    wait_for("the app said nothing", || {
        let snap = core.snapshot();
        said = snap.notice.as_ref().filter(|notice| notice.number > before).cloned();
        said.is_some()
    });
    said.map_or((false, String::new()), |notice| (notice.good, notice.detail))
}

/// The answer of the node process to one line.
fn answer(nodes: &mut Supervisor, id: u32, line: &str) -> ChildStatus {
    nodes.take_answers();
    nodes.send(id, line).unwrap();
    let mut answered = None;
    wait_for("the node said nothing", || {
        answered = nodes.take_answers().into_iter().find(|(from, _)| *from == id);
        answered.is_some()
    });
    answered.unwrap().1
}

fn own_coins(core: &Core) -> Vec<String> {
    let locks = core.snapshot().node.locks.clone();
    locks.iter().map(|coin| olc_app::core::host_locks::name_of_coin(&coin.point)).collect()
}

/// Pays to a lock from the app and waits for a block to hold it.
fn lock_coins(core: &Core, nodes: &mut Supervisor, spec: &NodeSpec, words: &str) -> String {
    let (good, detail) = told(core, LockOrder::Pay { words: words.into(), amount: "5".into() });
    assert!(good, "{detail}");
    let coin = detail.rsplit(' ').next().unwrap().to_string();
    wait_for("no block took the coins", || own_coins(core).contains(&coin));
    let height = core.snapshot().node.height;
    wait_for("the node never saw it", || {
        nodes.view(spec).status.is_some_and(|status| status.height >= height)
    });
    coin
}

fn balance(nodes: &mut Supervisor, spec: &NodeSpec) -> u64 {
    nodes.view(spec).status.map_or(0, |status| status.balance)
}

#[test]
fn a_hash_lock_and_two_of_two_are_paid_and_spent_between_real_nodes() {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("lock_nodes");
    let _ = std::fs::remove_dir_all(&root);
    let paths = Paths { root: root.join("app") };
    let port = 24_000 + (std::process::id() % 2_000) as u16;
    let core = app(&paths, port);
    wait_for("the app has no port", || core.snapshot().node.port.is_some());
    let address = format!("127.0.0.1:{}", core.snapshot().node.port.unwrap());

    let board_paths = Paths { root: root.join("board") };
    let file = save_coin(&board_paths, &coin()).unwrap();
    let mut board = Preset::Esp32S3.spec(200 + std::process::id() % 400);
    board.wallet = WalletChoice::Own;
    board.mining = false;
    let id = board.id;
    let mut nodes = Supervisor::with_program(board_paths, program());
    nodes.start(&board, &file, &[address]).unwrap();
    wait_for("the board-sized node has no coins to read", || {
        core.snapshot().node.spendable >= 20 * olc_core::coin::COIN
            && nodes.view(&board).status.is_some_and(|status| status.peers > 0)
    });

    // A hash lock: the wrong words change nothing, and the secret moves the coins.
    let coin = lock_coins(&core, &mut nodes, &board, &hash_lock_for(SECRET));
    let wrong = answer(&mut nodes, id, &format!("lockspend {coin} own \"not the words\""));
    assert_eq!(wrong.error.as_deref(), Some("lock.not_open"), "the node refuses");
    let refused = told(&core, LockOrder::Spend {
        coin: coin.clone(),
        to: String::new(),
        unlock: "\"not the words\"".into(),
    });
    assert!(!refused.0, "the app refuses");
    assert!(own_coins(&core).contains(&coin), "the coin stays");

    let right = answer(&mut nodes, id, &format!("lockspend {coin} own \"{SECRET}\""));
    assert!(right.paid.is_some(), "{right:?}");
    wait_for("the secret never paid", || balance(&mut nodes, &board) > 0);
    wait_for("the coin never left", || !own_coins(&core).contains(&coin));
    let after_hash = balance(&mut nodes, &board);

    // Two of two: one signature is not enough, and the spend passes between the nodes as text.
    let public = nodes.view(&board).status.unwrap().public;
    let own = core.snapshot().node.keys[0].clone();
    let lock = two_of_two_for(&own.public, &public).unwrap();
    let coin = lock_coins(&core, &mut nodes, &board, &lock);
    let unlock = core.snapshot().node.locks.iter().find(|row| {
        olc_app::core::host_locks::name_of_coin(&row.point) == coin
    }).unwrap().unlock.clone();
    assert!(unlock.starts_with("sig:"), "the preset's unlock comes ready: {unlock}");

    let blank = format!("0x{0} 0x{0}", "11".repeat(64));
    let forged = answer(&mut nodes, id, &format!("lockspend {coin} own {blank}"));
    assert_eq!(forged.error.as_deref(), Some("lock.not_open"), "made-up signatures");

    let half = told(&core, LockOrder::Spend { coin: coin.clone(), to: String::new(), unlock });
    assert!(half.0, "{}", half.1);
    let waiting = core.snapshot().node.half_spend.clone();
    wait_for("the app keeps the spend", || core.snapshot().node.half_spend.is_some());
    assert!(waiting.is_none_or(|(_, who)| who.len() == 1));
    let text = core.snapshot().node.half_spend.clone().unwrap().0;
    let signed = answer(&mut nodes, id, &format!("locksign {text}"));
    assert!(signed.paid.is_some(), "{signed:?}");
    wait_for("both signatures never spent it", || !own_coins(&core).contains(&coin));
    let spent_to_app = core.snapshot().node.balance;
    assert!(spent_to_app > 0);
    assert_eq!(balance(&mut nodes, &board), after_hash, "this spend pays the wallet that started it");

    assert!(nodes.view(&board).ended.is_none(), "the node stays within its memory");
    nodes.stop_all();
}
