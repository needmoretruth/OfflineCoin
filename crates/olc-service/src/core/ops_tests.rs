//! What a person hears when an order fails: plain words chosen by what went wrong.
use super::service::{Core, CoreLaunch};
use super::{coin_folder, Command, Host, LockOrder, Personal, Plan};
use crate::settings::{Paths, Settings};
use olc_core::CoinConfig;
use olc_node::{Card, Rank};
use olc_store::{load_wallet, NodeSettings};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

/// Folders whose chain may not open yet. On a real chain that takes seconds, and here it takes as
/// long as a scene needs it to.
static GATES: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// Holds the node's thread where a long chain would hold it.
pub fn wait_at_gate(folder: &Path) {
    while GATES.lock().unwrap_or_else(PoisonError::into_inner).iter().any(|gate| gate == folder) {
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Opens by itself when a test ends, since a host's thread must finish for the host to go.
struct Gate(PathBuf);

impl Gate {
    fn close(folder: &Path) -> Self {
        GATES.lock().unwrap_or_else(PoisonError::into_inner).push(folder.to_path_buf());
        Self(folder.to_path_buf())
    }

    fn open(&self) {
        GATES.lock().unwrap_or_else(PoisonError::into_inner).retain(|gate| *gate != self.0);
    }
}

impl Drop for Gate {
    fn drop(&mut self) {
        self.open();
    }
}

const SOMEONE: &str = "olc7fb614d9675c5f5b9851c5026632731f89ac8683";
const LINKS: &str = "wifi,wired,bluetooth,espnow";
const WAIT: Duration = Duration::from_secs(20);

fn folder(name: &str) -> PathBuf {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.scratch/tests/ops").join(name);
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn host(root: &Path) -> Host {
    let plan = Plan {
        coin: CoinConfig::olc(),
        folder: root.join("chain"),
        wallet_folder: root.join("wallet"),
        bind_host: "127.0.0.1".into(),
        port: 7400,
        port_tries: 90,
        peers: Vec::new(),
        cap: 0,
        mining: false,
        card: Card::default(),
        storage: None,
    };
    let host = Host::start(plan, None).unwrap();
    opened(&host);
    host
}

fn opened(host: &Host) {
    let started = Instant::now();
    while host.opening() {
        assert!(started.elapsed() < WAIT, "the chain never opened");
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn text(path: &Path) -> String {
    path.to_str().unwrap().to_string()
}

#[test]
fn a_chain_on_its_way_in_shows_as_loading() {
    let root = folder("loading");
    let host = host(&root);
    let seen = (0..20).any(|round| {
        host.switch(CoinConfig::olc(), root.join(format!("chain-{round}")), Vec::new(), None);
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(1) {
            if host.view().loading {
                return true;
            }
        }
        false
    });
    assert!(seen, "the node opened its chains and never said so");
    let started = Instant::now();
    while host.view().loading && started.elapsed() < Duration::from_secs(10) {
        std::thread::yield_now();
    }
    assert!(!host.view().loading, "the word stayed after the chain was in");
}

/// The engine a host starts on is thrown away, so nothing may be done to it. It refuses, and the
/// person hears, rather than a setting going with it.
#[test]
fn nothing_lands_on_an_engine_about_to_go() {
    let root = folder("about-to-go");
    let chain = root.join("chain-next");
    let gate = Gate::close(&chain);
    let host = host(&root);
    host.switch(CoinConfig::olc(), chain.clone(), Vec::new(), None);
    assert!(host.opening());

    let keys = load_wallet(&root.join("wallet")).unwrap().keys().count();
    assert!(!host.obey(Personal::NewKey).2, "a key went onto an engine that was going");
    assert!(!host.obey(Personal::Watch(SOMEONE.into())).2);
    assert!(!host.obey(Personal::SetMineNote("lost".into())).2);
    assert!(host.pay(SOMEONE, 1).is_err());
    assert!(host.pay_lock("HASH 0x00 EQUAL", "1").is_err());
    assert_eq!(load_wallet(&root.join("wallet")).unwrap().keys().count(), keys);

    gate.open();
    opened(&host);
    assert!(host.obey(Personal::NewKey).2);
    assert_eq!(load_wallet(&root.join("wallet")).unwrap().keys().count(), keys + 1);
}

fn core_on(root: &Path) -> (Core, Gate, PathBuf) {
    let paths = Paths { root: root.to_path_buf() };
    let chain = paths.coin(&coin_folder(&CoinConfig::olc()));
    let gate = Gate::close(&chain);
    let settings = Settings { port: 7500, setup_done: false, ..Settings::default() };
    let launch = CoreLaunch { paths, fleet: None, usb: false, node_program: Some("/bin/true".into()) };
    (Core::start(launch, settings).unwrap(), gate, chain)
}

fn until(core: &Core, what: &str, done: impl Fn(&super::Snapshot) -> bool) {
    let started = Instant::now();
    while !done(&core.snapshot()) {
        assert!(started.elapsed() < WAIT, "{what} never happened");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// A person who changes things while a chain loads gets them, in the order they came, on the chain
/// that stays, and the app says it is waiting.
#[test]
fn what_a_person_does_during_a_load_is_there_afterwards() {
    let root = folder("during-a-load");
    let (core, gate, chain) = core_on(&root);
    until(&core, "the first look", |snap| snap.node.loading);
    core.send(Command::Own(Personal::NewKey));
    core.send(Command::Own(Personal::Watch(SOMEONE.into())));
    core.send(Command::Own(Personal::SetMineNote("kept".into())));
    core.send(Command::Own(Personal::SetMiner(SOMEONE.into())));
    core.send(Command::Own(Personal::SetRecent(Some(5_000))));
    core.send(Command::Own(Personal::SetMerging(false)));
    core.send(Command::Own(Personal::SetLinks(Rank::parse(LINKS).unwrap())));
    core.send(Command::Lock(LockOrder::Pay { words: "HASH 0x00 EQUAL".into(), amount: "1".into() }));
    until(&core, "the word that it waits", |snap| {
        snap.notice.as_ref().is_some_and(|notice| notice.key == "notice.held")
    });
    assert!(core.snapshot().node.loading);

    gate.open();
    until(&core, "the chain", |snap| !snap.node.loading && snap.node.coin.is_some());
    until(&core, "the orders", |snap| {
        snap.node.keys.len() == 2 && snap.node.recent == Some(5_000) && !snap.node.merging
    });
    let view = core.snapshot();
    assert_eq!(view.node.watched.first().map(|watched| watched.0.as_str()), Some(SOMEONE));
    assert_eq!(view.node.mine_note, "kept");
    assert_eq!(view.node.miner, SOMEONE);
    assert_eq!(view.node.links.text(), LINKS);
    let saved = NodeSettings::load(&chain);
    assert_eq!(saved.watched, [SOMEONE.to_string()]);
    assert_eq!(saved.note, "kept");
    assert_eq!(saved.miner.as_deref(), Some(SOMEONE));
    assert_eq!(saved.recent, Some(5_000));
    assert!(saved.no_merge);
    assert_eq!(saved.links, LINKS);
    assert_eq!(load_wallet(&root.join("node")).unwrap().keys().count(), 2);
}

/// Closing the window while a chain loads takes nothing with it.
#[test]
fn closing_during_a_load_finishes_what_waits() {
    let root = folder("close-during-a-load");
    let (core, gate, chain) = core_on(&root);
    until(&core, "the first look", |snap| snap.node.loading);
    core.send(Command::Own(Personal::SetMineNote("kept".into())));
    let closing = core.clone();
    let closed = std::thread::spawn(move || closing.finish());
    std::thread::sleep(Duration::from_millis(200));
    assert!(!closed.is_finished(), "the app closed with an order waiting");
    gate.open();
    closed.join().unwrap();
    assert_eq!(NodeSettings::load(&chain).note, "kept");
}

/// A setting that restarts the node leaves a payment still waiting where it was.
#[test]
fn a_restart_of_the_node_keeps_payments_waiting() {
    let root = folder("keeps-payments");
    let mut coin = CoinConfig::olc();
    coin.name = "Waits".into();
    coin.initial_difficulty = 16;
    coin.rules.min_difficulty = 1;
    coin.rules.block_time_ms = 1_000;
    coin.rules.coinbase_maturity = 1;
    let host = Host::start(
        Plan { coin: coin.clone(), folder: root.join("chain"), wallet_folder: root.join("wallet"), bind_host: "127.0.0.1".into(), port: 7450, port_tries: 40, peers: Vec::new(), cap: 0, mining: false, card: Card::default(), storage: None },
        None,
    )
    .unwrap();
    opened(&host);
    for _ in 0..4 {
        let mut runner = host.runner.lock().unwrap();
        let now = runner.engine.chain().tip_header().time_ms + 1_000;
        while !runner.engine.mine(100_000, now) {}
    }
    let id = host.pay(SOMEONE, 1_000).unwrap();
    assert!(host.runner.lock().unwrap().engine.in_pool(&id));
    host.switch(coin, root.join("chain"), Vec::new(), None);
    opened(&host);
    assert!(host.runner.lock().unwrap().engine.in_pool(&id), "the payment went with the old engine");
}

#[test]
fn restoring_says_what_went_wrong() {
    let root = folder("restore");
    let host = host(&root);
    let file = root.join("wallet.backup");
    let path = text(&file);
    let order = Personal::BackUp { path: path.clone(), password: Some("right".into()) };
    assert_eq!(host.obey(order).0, "notice.backed_up");

    let wrong = Personal::Restore { path: path.clone(), password: Some("wrong".into()) };
    let (key, _, good) = host.obey(wrong);
    assert_eq!((key, good), ("notice.wrong_password", false));
    let none = Personal::Restore { path: path.clone(), password: None };
    assert_eq!(host.obey(none).0, "notice.needs_password");
    let right = Personal::Restore { path, password: Some("right".into()) };
    assert_eq!(host.obey(right).0, "notice.restored");

    let other = root.join("other.backup");
    std::fs::write(&other, "not a wallet").unwrap();
    let not_one = Personal::Restore { path: text(&other), password: None };
    assert_eq!(host.obey(not_one).0, "notice.not_a_backup");
}

#[test]
fn failures_name_the_order_that_failed() {
    let root = folder("failures");
    let host = host(&root);
    let (key, detail, good) = host.obey(Personal::ImportKey("not hex".into()));
    assert_eq!((key, good), ("notice.import_failed", false));
    assert!(!detail.is_empty(), "the expert word needs the cause");
    assert_eq!(host.obey(Personal::Watch("nobody".into())).0, "notice.bad_address");
    assert_eq!(host.obey(Personal::UseKey(9)).0, "notice.key_failed");
    let order = Personal::Sign { path: text(&root.join("missing")), save: None };
    assert_eq!(host.obey(order).0, "notice.offline_failed");
}

#[test]
fn a_payment_is_refused_where_there_is_nothing_to_spend() {
    let root = folder("offline");
    let host = host(&root);
    let address = host.view().address.clone();
    let order = Personal::Prepare {
        to: address,
        units: 1,
        fee: 0,
        save: Some(text(&root.join("out.payment"))),
    };
    assert_eq!(host.obey(order).0, "notice.offline_failed");
    assert!(!root.join("out.payment").exists());
}

/// The address other computers on the network reach this one by, when it has one.
fn address_on_the_network() -> Option<std::net::IpAddr> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    // Nothing is sent: connecting a UDP socket only picks the way out.
    socket.connect("192.0.2.1:9").ok()?;
    Some(socket.local_addr().ok()?.ip()).filter(|ip| !ip.is_loopback() && !ip.is_unspecified())
}

/// A person who stops letting others on the network connect expects it now, not at the next start.
#[test]
fn shutting_others_out_closes_the_door_at_once() {
    let root = folder("shut-out");
    let paths = Paths { root: root.clone() };
    let settings = Settings { port: 7530, setup_done: false, listen_lan: true, ..Settings::default() };
    let launch = CoreLaunch { paths, fleet: None, usb: false, node_program: Some("/bin/true".into()) };
    let core = Core::start(launch, settings.clone()).unwrap();
    until(&core, "a port", |snap| snap.node.port.is_some());
    let port = core.snapshot().node.port.unwrap_or_default();
    let Some(outside) = address_on_the_network() else {
        return;
    };
    let reach = |ip: std::net::IpAddr| std::net::TcpStream::connect((ip, port)).is_ok();
    assert!(reach(outside), "others cannot connect even with the switch on");

    core.send(Command::Save(Box::new(Settings { listen_lan: false, ..settings })));
    let started = Instant::now();
    while reach(outside) {
        assert!(started.elapsed() < WAIT, "the network still reaches this app");
        std::thread::sleep(Duration::from_millis(20));
    }
    // The new door opens a moment after the old one shuts, later on a slow machine.
    while !reach(std::net::Ipv4Addr::LOCALHOST.into()) {
        assert!(started.elapsed() < WAIT, "this computer lost its own way in");
        std::thread::sleep(Duration::from_millis(20));
    }
}
