//! What a person hears when an order fails: plain words chosen by what went wrong.
use super::{Host, Personal, Plan};
use olc_core::CoinConfig;
use olc_node::Card;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

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
    Host::start(plan, None).unwrap()
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
