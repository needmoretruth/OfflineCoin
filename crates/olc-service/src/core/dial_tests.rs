//! An address a person adds by hand is dialled wherever it is, this computer included.
use super::service::{Core, CoreLaunch};
use super::{Command, Host, Plan};
use crate::settings::{Paths, Settings};
use olc_core::CoinConfig;
use olc_node::Card;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const WAIT: Duration = Duration::from_secs(20);

fn folder(name: &str) -> PathBuf {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.scratch/tests/dial").join(name);
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap();
    path
}

/// Another node on this computer, listening on 127.0.0.1 only.
fn other_node(root: &Path, port: u16) -> (Host, String) {
    let plan = Plan {
        coin: CoinConfig::olc(),
        folder: root.join("other-chain"),
        wallet_folder: root.join("other-wallet"),
        bind_host: "127.0.0.1".into(),
        port,
        port_tries: 30,
        peers: Vec::new(),
        cap: 0,
        mining: false,
        card: Card::default(),
        storage: None,
    };
    let host = Host::start(plan, None).unwrap();
    let address = format!("127.0.0.1:{}", host.port.unwrap());
    (host, address)
}

fn core(root: &Path, port: u16, added: Vec<String>) -> Core {
    let paths = Paths { root: root.join("core") };
    let settings = Settings {
        port,
        setup_done: false,
        listen_lan: false,
        scan_lan: false,
        added,
        ..Settings::default()
    };
    let launch = CoreLaunch { paths, fleet: None, usb: false, node_program: Some("/bin/true".into()) };
    Core::start(launch, settings).unwrap()
}

fn connected(other: &Host) {
    let started = Instant::now();
    while other.view().peers == 0 {
        assert!(started.elapsed() < WAIT, "the added node was never dialled");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn an_address_added_earlier_is_dialled_at_start() {
    let root = folder("at-start");
    let (other, address) = other_node(&root, 7570);
    let core = core(&root, 7600, vec![address]);
    connected(&other);
    core.finish();
}

#[test]
fn an_address_added_now_is_dialled_now() {
    let root = folder("now");
    let (other, address) = other_node(&root, 7630);
    let core = core(&root, 7660, Vec::new());
    core.send(Command::AddAddress(address));
    connected(&other);
    core.finish();
}

#[test]
fn this_nodes_own_address_is_left_alone() {
    let mut settings = Settings { port: 7100, ..Settings::default() };
    settings.added = vec!["127.0.0.1:7100".into(), "localhost:7100".into(), "127.0.0.1:7101".into()];
    settings.added.push("127.0.0.1:7102".into());
    settings.peers = vec!["10.0.0.2:7000".into(), "127.0.0.1:7101".into()];
    assert_eq!(settings.to_dial(Some(7100)), ["10.0.0.2:7000", "127.0.0.1:7101", "127.0.0.1:7102"]);
    assert_eq!(settings.to_dial(None).len(), 5);
}
