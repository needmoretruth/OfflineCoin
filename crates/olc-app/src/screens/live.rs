//! Runs on the real coin, with the boards and desktop nodes of this computer: the switch to the
//! second transaction version, its lock time and relative lock, and a fork that takes neither
//! side's payment from the other. They wait on real blocks, so they run only when `OLC_LIVE`
//! names a running OLC node to start from, and print what they saw.
use crate::core::{Command, Core, CoreLaunch, LockOrder, Notice, Personal};
use crate::i18n::Lang;
use crate::settings::{Paths, Settings};
use olc_wallet::Unsigned;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const BLOCK_WAIT: Duration = Duration::from_secs(1);
/// What a node of the live runs mines at, so the boards keep finding blocks too.
pub(super) const LIVE_CAP: u64 = 2_000;

pub(super) fn live_peer() -> Option<String> {
    std::env::var("OLC_LIVE").ok()
}

pub(super) fn live_folder() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.scratch/live")
}

/// The data folder of one node of a live run, kept from the run before so its coins stay.
pub(super) fn live_root_kept(name: &str) -> PathBuf {
    let root = live_folder().join(name);
    fs::create_dir_all(&root).unwrap();
    root
}

/// A fresh data folder for one node of a live run.
pub(super) fn live_root(name: &str) -> PathBuf {
    let root = live_folder().join(name);
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    root
}

pub(super) fn live_settings(port: u16, peers: &[String]) -> Settings {
    Settings {
        setup_done: true,
        scan_lan: false,
        broadcast: false,
        listen_lan: false,
        port,
        peers: peers.to_vec(),
        language: Lang::Ko,
        hash_cap: LIVE_CAP,
        ..Settings::default()
    }
}

pub(super) fn start_core(root: &Path, settings: &Settings) -> Core {
    let launch = CoreLaunch { paths: Paths { root: root.to_path_buf() }, fleet: None, usb: false, node_program: None };
    Core::start(launch, settings.clone()).unwrap()
}

/// Prints a line of the run and keeps it in the run's log.
pub(super) fn tell(line: &str) {
    let stamp = chrono_free_stamp();
    println!("{stamp} {line}");
    let log = live_folder().join("run.log");
    if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(log) {
        let _ = writeln!(file, "{stamp} {line}");
    }
}

fn chrono_free_stamp() -> String {
    let seconds = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |since| since.as_secs());
    let day = (seconds + 9 * 3_600) % 86_400;
    format!("{:02}:{:02}:{:02}", day / 3_600, day / 60 % 60, day % 60)
}

#[track_caller]
pub(super) fn wait_for(what: &str, minutes: u64, mut done: impl FnMut() -> bool) {
    let started = Instant::now();
    while !done() {
        assert!(started.elapsed() < Duration::from_secs(minutes * 60), "waited {minutes} minutes for {what}");
        std::thread::sleep(BLOCK_WAIT);
    }
    tell(&format!("{what}: {} s", started.elapsed().as_secs()));
}

/// Sends `command` and gives the notice it caused.
pub(super) fn order(core: &Core, command: Command) -> Notice {
    let before = core.snapshot().notice.as_ref().map_or(0, |notice| notice.number);
    core.send(command);
    let mut heard = None;
    wait_for("an answer", 2, || {
        heard = core.snapshot().notice.clone().filter(|notice| notice.number > before && notice.key != "notice.held");
        heard.is_some()
    });
    let notice = heard.unwrap();
    tell(&format!("  {} {} {}", notice.key, if notice.good { "ok" } else { "refused" }, notice.detail));
    notice
}

pub(super) fn height(core: &Core) -> u64 {
    core.snapshot().node.height
}

fn explore(core: &Core, text: &str) -> String {
    core.send(Command::Own(Personal::Explore(text.to_string())));
    std::thread::sleep(Duration::from_millis(800));
    core.snapshot().node.explored.text(text)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// The block a payment is in, once it is in one.
fn block_of(core: &Core, id: &str) -> Option<u64> {
    let lines = explore(core, id);
    // The last thing looked up stays on show when this one is not found, so it must name `id`.
    if !lines.contains(id) {
        return None;
    }
    let line = lines.lines().find(|line| line.starts_with("in block"))?;
    line.split_whitespace().find_map(|word| word.replace(',', "").parse().ok())
}

/// A payment from this node's own coins, written to a file and signed there, with `edit` done to
/// it first. Gives the id of the unsigned payment and the signed file.
fn signed_file(core: &Core, root: &Path, name: &str, to: &str, edit: impl FnOnce(&mut serde_json::Value)) -> (String, String) {
    let unsigned = root.join(format!("{name}.unsigned.json")).display().to_string();
    let signed = root.join(format!("{name}.signed.json")).display().to_string();
    let prepared = order(core, Command::Own(Personal::Prepare { to: to.into(), units: 100_000, fee: 1_000, save: Some(unsigned.clone()) }));
    assert!(prepared.good, "{prepared:?}");
    let mut value: serde_json::Value = serde_json::from_str(&fs::read_to_string(&unsigned).unwrap()).unwrap();
    edit(&mut value);
    fs::write(&unsigned, value.to_string()).unwrap();
    let read: Unsigned = serde_json::from_value(value["unsigned"].clone()).unwrap();
    let made = order(core, Command::Own(Personal::Sign { path: unsigned, save: Some(signed.clone()) }));
    assert!(made.good, "{made:?}");
    (hex(&read.tx.id()), signed)
}

fn broadcast(core: &Core, file: &str) -> Notice {
    order(core, Command::Own(Personal::Broadcast(file.to_string())))
}

/// The id a broadcast answer names.
fn sent_id(notice: &Notice) -> String {
    notice.detail.split('"').nth(3).unwrap_or_default().to_string()
}

fn address_of(core: &Core) -> String {
    core.snapshot().node.address.clone()
}

/// The second transaction version beside the first in one block, an id known before signing, a
/// lock time, a relative lock, and a fork that takes neither side's payments. OLC has taken both
/// versions since its genesis.
#[test]
#[ignore = "needs the real coin: set OLC_LIVE to a running node"]
fn live_versions_and_a_fork() {
    let Some(peer) = live_peer() else { return };
    let (v_root, f_root) = (live_root_kept("V"), live_root("F"));
    let mut v_settings = live_settings(7860, std::slice::from_ref(&peer));
    v_settings.mine_on_start = true;
    let v = start_core(&v_root, &v_settings);
    let f = start_core(&f_root, &live_settings(7870, &[peer, "127.0.0.1:7860".into()]));
    wait_for("both wallets open", 2, || !address_of(&v).is_empty() && !address_of(&f).is_empty());
    let (v_address, f_address) = (address_of(&v), address_of(&f));
    tell(&format!("V {v_address}  F {f_address}"));
    // Two mined coins, so the two payments of one block need no coin still waiting for a block.
    wait_for("V has coins to spend", 40, || v.snapshot().node.spendable >= 100 * olc_core::coin::COIN);
    v.send(Command::SetMining(false));

    tell("1. A version 1 and a version 2 payment in one block");
    let _ = order(&v, Command::Own(Personal::SetVersionOne(true)));
    let old = order(&v, Command::Pay { to: f_address.clone(), amount: "3".into() });
    let _ = order(&v, Command::Own(Personal::SetVersionOne(false)));
    let new = order(&v, Command::Pay { to: f_address.clone(), amount: "4".into() });
    assert!(old.good && new.good);
    let sent = |core: &Core| -> Vec<String> {
        let rows = core.snapshot().node.activity.clone();
        rows.iter().filter(|row| row.kind == crate::core::ActivityKind::Sent).map(|row| hex(&row.id)).collect()
    };
    wait_for("both payments listed", 1, || sent(&v).len() >= 2);
    let ids = sent(&v);
    tell(&format!("  sent {ids:?}"));
    let mut blocks = Vec::new();
    wait_for("both payments in blocks", 20, || {
        blocks = ids.iter().take(2).map(|id| block_of(&v, id)).collect();
        blocks.iter().all(Option::is_some)
    });
    for id in ids.iter().take(2) {
        tell(&format!("  {}", explore(&v, id).lines().filter(|line| line.starts_with("payment") || line.starts_with("in block")).collect::<Vec<_>>().join(" | ")));
    }
    tell(&format!("  blocks {blocks:?}, tip {}", height(&v)));

    tell("2. A version 2 id is known before the signatures");
    let _ = order(&v, Command::Own(Personal::Watch(v_address.clone())));
    let (before, file) = signed_file(&v, &v_root, "early-id", &f_address, |_| {});
    let answer = broadcast(&v, &file);
    tell(&format!("  unsigned {before}, sent {}", sent_id(&answer)));
    assert_eq!(before, sent_id(&answer));
    wait_for("that payment in a block", 10, || block_of(&v, &before).is_some());

    tell("3. A lock time holds a payment until its block");
    let hold = height(&v) + 3;
    let (_, file) = signed_file(&v, &v_root, "lock-time", &f_address, |value| {
        value["unsigned"][0]["lock_time"] = serde_json::json!(hold);
        // As in Bitcoin, a lock time binds only a payment with an input that is not final.
        if let Some(inputs) = value["unsigned"][0]["inputs"].as_array_mut() {
            inputs.iter_mut().for_each(|input| input["sequence"] = serde_json::json!(u32::MAX - 1));
        }
    });
    assert!(!broadcast(&v, &file).good, "a payment for block {hold} went in early");
    wait_for("the lock time", 10, || height(&v) >= hold);
    let answer = broadcast(&v, &file);
    assert!(answer.good, "{answer:?}");
    let id = sent_id(&answer);
    wait_for("the held payment in a block", 10, || block_of(&v, &id).is_some());

    tell("4. A relative lock opens two blocks after its coins");
    let own = v_address.trim_start_matches("olc");
    let words = format!("2 CHECKSEQUENCEVERIFY DROP DUP HASH 0x{own} EQUALVERIFY CHECKSIG");
    let paid = order(&v, Command::Lock(LockOrder::Pay { words, amount: "2".into() }));
    assert!(paid.good, "{paid:?}");
    let coin = paid.detail.split_whitespace().last().unwrap_or_default().to_string();
    let txid = coin.split(':').next().unwrap_or_default().to_string();
    let mut at = None;
    wait_for("the locked coins in a block", 10, || {
        at = block_of(&v, &txid);
        at.is_some()
    });
    let at = at.unwrap_or_default();
    let spend = || LockOrder::Spend { coin: coin.clone(), to: String::new(), unlock: format!("sig:{v_address} key:{v_address}") };
    if height(&v) + 1 < at + 2 {
        let early = order(&v, Command::Lock(spend()));
        assert!(!early.good, "the relative lock opened a block early");
    }
    wait_for("two blocks on the locked coins", 10, || height(&v) + 1 >= at + 2);
    let opened = order(&v, Command::Lock(spend()));
    assert!(opened.good, "{opened:?}");
    tell(&format!("  locked in block {at}, opened for block {}", height(&v) + 1));

    tell("5. A fork takes neither side's payments");
    wait_for("F has its coins", 10, || f.snapshot().node.spendable >= 7 * olc_core::coin::COIN);
    let forked = order(&f, Command::Fork { at: None, changes: vec![("rules.reward".into(), "2".into())] });
    assert!(forked.good, "{forked:?}");
    let fork_at: u64 = forked.detail.parse().unwrap_or_default();
    tell(&format!("  the fork starts at block {fork_at}, F at {}", height(&f)));
    let (_, old_file) = signed_file(&v, &v_root, "version-one", &v_address, |value| {
        value["unsigned"][0]["version"] = serde_json::json!(1);
        // A payment that names no coin is the pair older builds wrote: payment and owners.
        if let Some(parts) = value["unsigned"].as_array_mut() {
            parts.truncate(2);
        }
    });
    let on_fork = broadcast(&f, &old_file);
    tell(&format!("  OLC version 1 payment on the fork: {}", on_fork.detail));
    assert!(!on_fork.good);
    assert!(broadcast(&v, &old_file).good, "OLC refused its own version 1 payment");
    let _ = order(&f, Command::Own(Personal::Watch(f_address.clone())));
    let (_, fork_file) = signed_file(&f, &f_root, "fork-payment", &f_address, |_| {});
    let on_olc = broadcast(&v, &fork_file);
    tell(&format!("  fork payment on OLC: {}", on_olc.detail));
    assert!(!on_olc.good);
    assert!(broadcast(&f, &fork_file).good, "the fork refused its own payment");
    tell(&format!("done: V at {}, F at {}", height(&v), height(&f)));
}
