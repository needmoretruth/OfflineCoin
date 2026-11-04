//! The password on a running node, as a person meets it: locked after a restart, open for the
//! time picked and then locked again, and a wrong password that changes nothing.
use super::*;
use crate::core::{ContactOrder, Personal, Plan, WalletOrder};
use olc_core::CoinConfig;
use olc_node::Card;
use olc_store::load_wallet;
use std::fs;
use std::path::PathBuf;

const PASSWORD: &str = "a long password";
const WAIT: Duration = Duration::from_secs(20);

fn folder(name: &str) -> PathBuf {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.scratch/tests/lock").join(name);
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).unwrap();
    path
}

fn host(root: &Path) -> Host {
    let plan = Plan {
        coin: CoinConfig::olc(),
        folder: root.join("chain"),
        wallet_folder: root.join("wallet"),
        bind_host: "127.0.0.1".into(),
        port: 7600,
        port_tries: 90,
        peers: Vec::new(),
        cap: 0,
        mining: false,
        card: Card::default(),
        storage: None,
        shares_wallet: false,
    };
    let host = Host::start(plan, None).unwrap();
    let started = Instant::now();
    while host.opening() {
        assert!(started.elapsed() < WAIT, "the chain never opened");
        std::thread::sleep(Duration::from_millis(2));
    }
    host
}

fn password(order: PasswordOrder) -> Personal {
    Personal::Wallet(WalletOrder::Password(order))
}

fn unlock(text: &str, seconds: u64) -> Personal {
    password(PasswordOrder::Unlock { password: Password(text.into()), seconds })
}

fn sign() -> Personal {
    Personal::Wallet(WalletOrder::SignMessage { address: String::new(), message: "hello".into() })
}

/// Waits until the published view says what `ready` wants.
fn view_until(host: &Host, ready: impl Fn(&LockView) -> bool) -> LockView {
    let started = Instant::now();
    loop {
        let lock = host.view().wallet.lock;
        if ready(&lock) {
            return lock;
        }
        assert!(started.elapsed() < WAIT, "the view never said so: {lock:?}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn sealed_host(name: &str) -> (PathBuf, Host) {
    let root = folder(name);
    let host = host(&root);
    let (key, _, good) = host.obey(password(PasswordOrder::Encrypt(Password(PASSWORD.into()))));
    assert!(good, "{key}");
    (root, host)
}

#[test]
fn the_wallet_locks_again_when_its_open_time_runs_out() {
    let (_root, host) = sealed_host("time");
    view_until(&host, |lock| lock.sealed && lock.open_for.is_none());
    assert_eq!(host.obey(sign()).0, "notice.wallet_locked");
    assert_eq!(host.obey(Personal::Wallet(WalletOrder::ShowWords)).0, "notice.wallet_locked");
    let (key, seconds, good) = host.obey(unlock(PASSWORD, 2));
    assert!(good && key == "notice.wallet_opened" && seconds == "2");
    view_until(&host, |lock| lock.open_for.is_some());
    assert!(host.obey(sign()).2, "an open wallet signs");
    assert!(host.obey(Personal::Wallet(WalletOrder::ShowWords)).2);
    view_until(&host, |lock| lock.open_for.is_none());
    assert_eq!(host.obey(sign()).0, "notice.wallet_locked", "the time ran out and the keys went with it");
    assert!(host.obey(unlock(PASSWORD, 300)).2);
    assert!(host.obey(password(PasswordOrder::Lock)).2);
    assert_eq!(host.obey(sign()).0, "notice.wallet_locked", "locking now does not wait for the time");
}

#[test]
fn a_wrong_password_opens_nothing_and_changes_nothing() {
    let (root, host) = sealed_host("wrong");
    let file = root.join("wallet").join("wallet.json");
    let before = fs::read(&file).unwrap();
    assert_eq!(host.obey(unlock("not it", 300)).0, "notice.wrong_wallet_password");
    let change = PasswordOrder::Change { old: Password("not it".into()), new: Password("new".into()) };
    assert_eq!(host.obey(password(change)).0, "notice.wrong_wallet_password");
    assert_eq!(view_until(&host, |lock| lock.wrong == 2).open_for, None);
    assert_eq!(fs::read(&file).unwrap(), before);
    assert_eq!(host.obey(sign()).0, "notice.wallet_locked");
}

/// A restart finds the wallet locked. It still knows its addresses, takes contacts and names,
/// makes a backup that keeps its keys sealed, and refuses a key it could not keep.
#[test]
fn an_encrypted_wallet_starts_locked_after_a_restart() {
    let (root, before) = sealed_host("restart");
    view_until(&before, |lock| lock.sealed);
    let address = before.view().address;
    assert!(!address.is_empty());
    drop(before);
    let host = host(&root);
    let lock = view_until(&host, |lock| lock.sealed);
    assert_eq!(lock.open_for, None);
    assert_eq!(host.view().address, address);
    assert!(host.view().wallet.has_words, "sealed words are still words");
    let add = ContactOrder::Add { name: "Mina".into(), address: address.clone() };
    assert!(host.obey(Personal::Wallet(WalletOrder::Contact(add))).2);
    let keys = load_wallet(&root.join("wallet")).unwrap().key_count();
    assert_eq!(host.obey(Personal::Wallet(WalletOrder::NewAddress)).0, "notice.wallet_locked");
    assert_eq!(host.obey(Personal::NewKey).0, "notice.wallet_locked");
    assert_eq!(load_wallet(&root.join("wallet")).unwrap().key_count(), keys);
    let backup = root.join("locked.backup");
    assert!(host.obey(Personal::BackUp { path: backup.display().to_string(), password: None }).2);
    let back = olc_store::restore(&backup, Some(PASSWORD)).unwrap();
    assert!(back.is_open());
    assert_eq!(back.contacts.len(), 1);
    assert!(host.obey(unlock(PASSWORD, 300)).2);
    assert!(host.obey(Personal::NewKey).2, "an open wallet seals a new key with the rest");
    assert_eq!(load_wallet(&root.join("wallet")).unwrap().key_count(), keys + 1);
}
