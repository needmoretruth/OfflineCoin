//! A value a person set stays as they set it, whatever else they press afterwards.
use super::super::{Command, Personal};
use super::{Core, CoreLaunch};
use crate::settings::{Device, Paths, Settings};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const WAIT: Duration = Duration::from_secs(20);

fn folder(name: &str) -> PathBuf {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.scratch/tests/kept").join(name);
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap();
    path
}

/// What the app holds at its start: the same file the core read, kept to this computer.
fn app_copy(port: u16) -> Settings {
    Settings {
        setup_done: true,
        listen_lan: false,
        scan_lan: false,
        broadcast: false,
        port,
        // Whether it mines is what these look at, not how fast; other tests share the computer.
        hash_cap: 1_000,
        mining_threads: 1,
        ..Settings::default()
    }
}

fn core_on(root: &Path, settings: &Settings) -> Core {
    let paths = Paths { root: root.to_path_buf() };
    let launch = CoreLaunch { paths, fleet: None, usb: false, node_program: Some("/bin/true".into()) };
    let core = Core::start(launch, settings.clone()).unwrap();
    until(&core, "the chain", |snap| !snap.node.loading && snap.node.coin.is_some());
    core
}

fn until(core: &Core, what: &str, done: impl Fn(&super::super::Snapshot) -> bool) {
    let started = Instant::now();
    while !done(&core.snapshot()) {
        assert!(started.elapsed() < WAIT, "{what} never happened");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn saved(root: &Path) -> Settings {
    Settings::load(root)
}

/// The console's cap, then a theme picked on the Settings page from the app's older copy.
#[test]
fn a_cap_set_elsewhere_survives_saving_another_setting() {
    let root = folder("cap-survives");
    let copy = Settings { hash_cap: 40_000, ..app_copy(7611) };
    let core = core_on(&root, &copy);
    core.send(Command::SetCap(5_000));
    until(&core, "the console's cap", |snap| snap.node.cap == 5_000);
    core.send(Command::Save(Box::new(Settings { theme: crate::settings::Theme::Dark, ..copy })));
    core.finish();
    assert_eq!(saved(&root).theme, crate::settings::Theme::Dark);
    assert_eq!(saved(&root).hash_cap, 5_000, "the cap went back to the app's old copy");
    // The node's own view follows a few turns later, and never goes back.
    std::thread::sleep(Duration::from_millis(600));
    until(&core, "the console's cap after saving", |snap| snap.node.cap == 5_000);
}

#[test]
fn start_and_stop_leave_mine_when_the_app_opens_alone() {
    let root = folder("start-stop");
    let core = core_on(&root, &app_copy(7613));
    core.send(Command::SetMining(true));
    until(&core, "mining", |snap| snap.node.mining);
    core.send(Command::SetMining(false));
    core.finish();
    core.send(Command::SetMining(true));
    core.finish();
    assert!(!saved(&root).mine_on_start, "Start on Home turned on mining at every opening");
}

#[test]
fn the_console_word_mining_acts_in_the_app() {
    let root = folder("console-mining");
    let core = core_on(&root, &app_copy(7615));
    core.send(Command::Own(Personal::SetMining(true)));
    until(&core, "mining after `mining on`", |snap| snap.node.mining);
    // The app's Settings page reads it here, so its switch is not left behind.
    until(&core, "the app hearing mining at opening", |snap| snap.mine_on_start);
    core.finish();
    assert!(saved(&root).mine_on_start);
}

#[test]
fn several_miners_chosen_later_are_made() {
    let root = folder("several-later");
    let copy = app_copy(7617);
    let core = core_on(&root, &copy);
    core.send(Command::Save(Box::new(Settings { device: Device::SeveralNodes, ..copy })));
    core.finish();
    assert_eq!(saved(&root).nodes.len(), 3, "no miners came of choosing several");
}

#[test]
fn a_computer_does_not_merge_coins_by_itself() {
    let root = folder("no-merge");
    let core = core_on(&root, &app_copy(7619));
    assert!(!core.snapshot().node.merging, "the node merges coins without being asked");
    core.send(Command::Own(Personal::SetMerging(true)));
    until(&core, "merging after the switch", |snap| snap.node.merging);
}
