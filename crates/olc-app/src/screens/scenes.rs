//! The owner's scenes, by clicking in the window on real nodes.
use super::{
    app, click, harness, node_program, quiet_settings, scratch, type_into, type_secret, wait_for_notice,
    wait_until, Window, SETTLE_STEPS, SIZES,
};
use crate::core::{coin_folder, save_coin, Book, Command, Host, Plan, Snapshot};
use crate::discovery::Entry;
use crate::i18n::Lang;
use crate::local::{is_running, Holding, NodeSpec, Preset, Supervisor};
use crate::settings::{Device, Paths, Settings};
use egui_kittest::kittest::Queryable;
use olc_core::{CoinConfig, FEWEST_RECENT};
use olc_node::Card;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Longer than a few rounds of asking, shorter than a look around.
const LIVE_WATCH: Duration = Duration::from_secs(16);
/// Two seconds between answers at a block a second, and some luck.
const MOST_BLOCKS_BEHIND: u64 = 9;

/// A new node first reports after its first second.
fn height_of(nodes: &mut Supervisor, spec: &NodeSpec) -> u64 {
    for _ in 0..100 {
        if let Some(status) = nodes.view(spec).status {
            return status.height;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("the node never reported");
}

fn row_of(snap: &Snapshot, name: &str) -> Option<Entry> {
    snap.groups
        .iter()
        .flat_map(|group| &group.chains)
        .flat_map(|chain| &chain.entries)
        .find(|entry| entry.name == name)
        .cloned()
}

/// Three questions and a new coin end on real node processes.
#[test]
fn the_first_run_starts_real_node_processes() {
    let settings = Settings { setup_done: false, port: 7310, ..quiet_settings() };
    let window = app(scratch("first-nodes"), settings, Some(node_program()));
    let mut harness = harness(window, SIZES[0]);
    click(&mut harness, "Just this computer");
    // A new coin starts easy, so its height moves while the table is watched.
    click(&mut harness, "New coin");
    type_into(&mut harness, 0, "Quick");
    harness.run_steps(2);
    click(&mut harness, "Next");
    click(&mut harness, "Start mining");
    assert!(harness.state_mut().settings_mut().setup_done);
    wait_until(&mut harness, |app| {
        let local = &app.snapshot().local;
        local.len() == 3 && local.iter().all(|view| view.status.is_some())
    });
    let pids: Vec<u32> = harness.state().snapshot().local.iter().filter_map(|view| view.pid).collect();
    assert_eq!(pids.len(), 3);
    assert!(pids.iter().copied().all(is_running));
    assert_ne!(pids[0], std::process::id());

    // The table and the nodes must agree at every moment, not only after a look around.
    let start = Instant::now();
    let mut heights = BTreeSet::new();
    while start.elapsed() < LIVE_WATCH {
        let snap = harness.state().snapshot();
        for view in &snap.local {
            let status = view.status.as_ref().expect("a running node");
            let row = row_of(&snap, &view.spec.name).expect("a row for every node");
            assert_eq!(row.height, Some(status.height));
            assert_eq!(row.age_secs, None);
            heights.insert(status.height);
        }
        std::thread::sleep(Duration::from_millis(300));
    }
    assert!(heights.len() > 1, "the chain never moved");
}

/// The owner's scene: one node full without mining, one pruned and mining, one light, all on one
/// chain, and each set in the window as a person would set it.
#[test]
fn each_node_is_the_kind_its_person_chose() {
    let settings = Settings { port: 7330, ..quiet_settings() };
    let window = app(scratch("kinds"), settings, Some(node_program()));
    let mut harness = harness(window, SIZES[1]);
    let mut coin = CoinConfig::olc();
    coin.name = "Kinds".into();
    coin.rules.block_time_ms = 1_000;
    coin.rules.min_difficulty = 100;
    coin.initial_difficulty = 100;
    harness.state().send(Command::Join { coin: Box::new(coin), address: None });
    harness.run_steps(2);
    let spec = |id: u32, mining, holding| NodeSpec {
        mining,
        holding,
        recent: 300,
        ..Preset::Custom.spec(id)
    };
    for node in [
        spec(10, false, Holding::Everything),
        spec(11, true, Holding::Recent),
        spec(12, false, Holding::Headers),
    ] {
        harness.state().send(Command::AddNode(Box::new(node)));
    }
    wait_until(&mut harness, |app| {
        let local = &app.snapshot().local;
        let at = |index: usize| local.get(index).and_then(|view| view.status.as_ref());
        let held = at(2).and_then(|status| status.headers).unwrap_or(0);
        let miner = at(1).is_some_and(|status| status.height >= 5 && status.hashrate > 0);
        local.len() == 3 && held >= 5 && miner
    });
    let snap = harness.state().snapshot().clone();
    let status: Vec<_> = snap.local.iter().filter_map(|view| view.status.clone()).collect();
    assert_eq!(status[0].headers, None);
    assert!(!status[0].mining, "a full node here only validates");
    assert!(status[1].mining);
    assert!(!status[2].mining);
    wait_until(&mut harness, |app| {
        let local = &app.snapshot().local;
        let height = |index: usize| local[index].status.as_ref().map_or(0, |status| status.height);
        height(0).abs_diff(height(2)) <= MOST_BLOCKS_BEHIND
    });

    // The same choice on this app's own node, which comes back as the other kind.
    harness.set_size(egui::vec2(1280.0, 1900.0));
    harness.state_mut().open("page.settings");
    harness.run_steps(SETTLE_STEPS);
    harness.get_by_label("Just a summary").click();
    wait_until(&mut harness, |app| {
        let node = &app.snapshot().node;
        node.height >= 5 && node.light.is_some_and(|light| light.held_headers >= 5)
    });
    harness.state_mut().open("page.home");
    harness.run_steps(SETTLE_STEPS);
    harness.get_by_label("This computer keeps no blocks, so it cannot mine.");
    harness.state_mut().open("page.records");
    harness.run_steps(SETTLE_STEPS);
    harness.get_by_label("This computer keeps only a summary. Keep more blocks to see them here.");
}

fn the_backup_in(root: &Path) -> Option<PathBuf> {
    let files = std::fs::read_dir(root).ok()?;
    let found = files.flatten().map(|entry| entry.path());
    found.into_iter().find(|path| path.extension().is_some_and(|word| word == "backup"))
}

/// The owner's scene: a note goes into a block and shows in the records, and a second address is
/// made, backed up and restored, all by clicking in the window on a real node.
#[test]
fn a_note_and_a_key_go_through_the_window() {
    let root = scratch("scene");
    let mut coin = CoinConfig::olc();
    coin.name = "Scene".into();
    coin.ticker = "SCN".into();
    coin.initial_difficulty = 200;
    coin.rules.min_difficulty = 1;
    save_coin(&Paths { root: root.clone() }, &coin).unwrap();
    let mut settings = quiet_settings();
    settings.coin = Some(coin_folder(&coin));
    settings.device = Device::ThisComputer;
    settings.mine_on_start = true;
    settings.port = 7360;
    let window = app(root.clone(), settings, Some(node_program()));
    let mut harness = harness(window, (1280, 1400));
    harness.state_mut().settings_mut().language = Lang::En;

    harness.state_mut().open("page.settings");
    wait_until(&mut harness, |app| app.snapshot().node.keys.len() == 1);
    harness.run_steps(2);
    harness.query_all_by_label("Open").next().expect("the addresses row").click();
    harness.run_steps(2);
    click(&mut harness, "New address");
    wait_until(&mut harness, |app| app.snapshot().node.keys.len() == 2);
    harness.run_steps(2);
    click(&mut harness, "Receive here");
    wait_until(&mut harness, |app| app.snapshot().node.keys[1].receiving);
    harness.run_steps(2);
    click(&mut harness, "Close");

    click(&mut harness, "Back up");
    type_secret(&mut harness, 0, "a long password");
    click(&mut harness, "Choose where to save");
    click(&mut harness, "Save here");
    wait_for_notice(&mut harness, "notice.backed_up");
    let backup = the_backup_in(&root).expect("a backup file");
    let file = std::fs::read_to_string(&backup).unwrap();
    assert!(!file.contains("\"keys\""), "a password hides the keys");

    click(&mut harness, "Restore");
    type_secret(&mut harness, 0, "a long password");
    click(&mut harness, "Choose the backup file");
    click(&mut harness, backup.file_name().and_then(|name| name.to_str()).unwrap());
    harness.query_all_by_label("Open").last().expect("the picker's own button").click();
    wait_for_notice(&mut harness, "notice.restored");

    harness.state_mut().open("page.records");
    wait_until(&mut harness, |app| app.snapshot().node.height > 2);
    harness.run_steps(2);
    type_into(&mut harness, 1, "written by hand");
    click(&mut harness, "Write");
    // The slip reads the note back before it goes.
    assert!(harness.query_all_by_label("written by hand").count() > 0);
    harness.query_all_by_label("Write").last().expect("the slip's own button").click();
    wait_until(&mut harness, |app| {
        app.snapshot().node.notes.iter().any(|note| note.text == "written by hand")
    });
    harness.run_steps(SETTLE_STEPS);
    assert!(harness.query_all_by_label_contains("written by hand").next().is_some());
}

/// A real node that mined `blocks` and dropped all but the fewest recent ones.
fn pruned_peer(root: &Path, coin: &CoinConfig, port: u16, blocks: u64) -> Host {
    let host = Host::start(
        Plan {
            coin: coin.clone(),
            folder: root.join("chain"),
            wallet_folder: root.join("wallet"),
            bind_host: "127.0.0.1".into(),
            port,
            port_tries: 1,
            peers: Vec::new(),
            cap: 0,
            mining: false,
            card: Card::default(),
            storage: None,
        },
        None,
    )
    .unwrap();
    host.set_recent(Some(FEWEST_RECENT)).unwrap();
    let block_time = coin.rules.block_time_ms;
    for _ in 0..blocks {
        let mut runner = host.runner.lock().unwrap();
        let now = runner.engine.chain().tip_header().time_ms + block_time;
        while !runner.engine.mine(100_000, now) {}
    }
    // Nothing a boot still reads is dropped, so the state is saved first, as a tick would.
    let mut runner = host.runner.lock().unwrap();
    runner.engine.chain_mut().save_now();
    runner.engine.chain_mut().set_recent(Some(FEWEST_RECENT));
    assert!(runner.engine.chain().held().oldest > 1);
    drop(runner);
    host
}

/// Nobody holds the first blocks, so the node asks, and goes on only after the person says yes.
#[test]
fn a_snapshot_waits_for_the_person_in_the_window() {
    let root = scratch("history");
    let mut coin = CoinConfig::olc();
    coin.name = "Cut".into();
    coin.ticker = "CUT".into();
    coin.initial_difficulty = 16;
    coin.rules.min_difficulty = 1;
    let peer = pruned_peer(&root.join("peer"), &coin, 7340, 700);
    let tip = peer.view().height;

    let app_root = root.join("app");
    save_coin(&Paths { root: app_root.clone() }, &coin).unwrap();
    let mut settings = quiet_settings();
    settings.coin = Some(coin_folder(&coin));
    settings.device = Device::ThisComputer;
    settings.mine_on_start = false;
    settings.port = 7341;
    settings.peers = vec!["127.0.0.1:7340".into()];
    let mut harness = harness(app(app_root, settings, Some(node_program())), SIZES[0]);
    harness.state_mut().settings_mut().language = Lang::En;
    harness.state_mut().open("page.coins");
    wait_until(&mut harness, |app| app.snapshot().node.needs_history.is_some());
    assert_eq!(harness.state().snapshot().node.height, 0, "nothing moves without a yes");
    harness.run_steps(SETTLE_STEPS);
    click(&mut harness, "Read what this means");
    click(&mut harness, "Trust them");
    wait_until(&mut harness, |app| app.snapshot().node.height >= tip);
    assert!(harness.state().snapshot().node.assumed_from.is_some());
}

/// A coin running on this computer is found, and joining it needs no file.
#[test]
fn a_found_coin_can_be_joined() {
    let root = scratch("join-other");
    let paths = Paths { root };
    let mut coin = CoinConfig::olc();
    coin.name = "Found".into();
    coin.ticker = "FND".into();
    coin.message = "Found genesis".into();
    coin.initial_difficulty = 15_000;
    coin.rules.retarget_interval = 0;
    let file = save_coin(&paths, &coin).unwrap();
    let mut other = Supervisor::with_program(paths, node_program());
    let spec = Preset::Esp32S3.spec(9);
    other.start(&spec, &file, &[]).unwrap();

    let settings = Settings { setup_done: false, port: 7320, ..quiet_settings() };
    let mut harness = harness(app(scratch("join"), settings, None), SIZES[0]);
    click(&mut harness, "ESP32 boards");
    wait_until(&mut harness, |app| {
        app.snapshot().groups.iter().any(|group| {
            group.identity.as_ref().is_some_and(|identity| identity.coin.name == "Found")
        })
    });
    harness.run_steps(2);
    click(&mut harness, "Found");
    click(&mut harness, "Not now");
    wait_until(&mut harness, |app| {
        let node = &app.snapshot().node;
        let name = node.coin.as_ref().map(|coin| coin.name.as_str());
        name == Some("Found") && node.peers > 0
    });

    // Until the new link answers once, the row still shows the last look around.
    wait_until(&mut harness, |app| {
        row_of(&app.snapshot(), &spec.name).is_some_and(|row| row.age_secs.is_none())
    });

    // A block about every second: a row that waits for a look around falls far behind.
    let start = Instant::now();
    let first = height_of(&mut other, &spec);
    let mut last = first;
    while start.elapsed() < LIVE_WATCH {
        let snap = harness.state().snapshot();
        last = height_of(&mut other, &spec);
        let row = row_of(&snap, &spec.name).expect("a row for the peer");
        let shown = row.height.expect("a height");
        assert_eq!(row.age_secs, None);
        assert!(last.saturating_sub(shown) <= MOST_BLOCKS_BEHIND, "{shown} against {last}");
        std::thread::sleep(Duration::from_millis(300));
    }
    assert!(last > first + 3, "the chain never moved");
    other.stop_all();
}

/// The console says the same as the buttons: the words go through the core's own table.
#[test]
fn the_console_answers_in_the_core_words() {
    let mut settings = quiet_settings();
    settings.mode = crate::settings::Mode::Expert;
    settings.port = 7370;
    let mut harness = harness(app(scratch("console"), settings, None), SIZES[0]);
    harness.state_mut().open("page.console");
    harness.run_steps(SETTLE_STEPS);
    wait_until(&mut harness, |app| !app.snapshot().node.address.is_empty());
    type_into(&mut harness, 0, "keys");
    click(&mut harness, "Run");
    let address = harness.state().snapshot().node.address.clone();
    wait_until_seen(&mut harness, &address);
}

fn wait_until_seen(harness: &mut Window, text: &str) {
    for _ in 0..100 {
        harness.run_steps(1);
        if harness.query_all_by_label_contains(text).next().is_some() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("{text} never showed");
}
