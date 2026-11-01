//! Tests of the whole window: every page in every language and theme, drawn by the real app on a
//! real core. Node processes in the pictures are real, and a real board appears only in a real
//! window, so these leave USB alone.
use crate::app::App;
use crate::core::{coin_folder, save_coin, Command, Core, CoreLaunch, Host, Plan, Snapshot};
use crate::discovery::Entry;
use crate::core::Book;
use crate::i18n::Lang;
use crate::local::{is_running, Holding, NodeSpec, Preset, Supervisor};
use crate::settings::{Device, Paths, Settings, Theme};
use egui::accesskit::Role;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use olc_core::{CoinConfig, FEWEST_RECENT};
use olc_node::Card;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const SIZES: [(u32, u32); 2] = [(1280, 800), (1920, 1080)];
const SETTLE_STEPS: usize = 4;
/// Longer than a few rounds of asking, shorter than a look around.
const LIVE_WATCH: Duration = Duration::from_secs(16);
/// Two seconds between answers at a block a second, and some luck.
const MOST_BLOCKS_BEHIND: u64 = 9;

fn scratch(name: &str) -> PathBuf {
    let folder = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../.scratch/tests")
        .join(name);
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).unwrap();
    folder
}

fn quiet_settings() -> Settings {
    Settings {
        setup_done: true,
        scan_lan: false,
        broadcast: false,
        port: 7300,
        ..Settings::default()
    }
}

fn app(root: PathBuf, settings: Settings, program: Option<PathBuf>) -> App {
    let launch = CoreLaunch {
        paths: Paths { root },
        fleet: None,
        usb: false,
        node_program: program,
    };
    let core = Core::start(launch, settings.clone()).unwrap();
    App::new(core, settings, None)
}

fn harness(app: App, size: (u32, u32)) -> Harness<'static, App> {
    let mut harness = Harness::builder()
        .with_size(egui::vec2(size.0 as f32, size.1 as f32))
        .build_ui_state(|ui, app: &mut App| app.draw(ui), app);
    harness.run_steps(SETTLE_STEPS);
    harness
}

#[test]
fn every_page_draws_in_both_languages() {
    let mut harness = harness(app(scratch("pages"), quiet_settings(), None), SIZES[0]);
    for title in harness.state().page_titles() {
        for language in [Lang::En, Lang::Ko] {
            harness.state_mut().settings_mut().language = language;
            harness.state_mut().open(title);
            harness.run_steps(2);
        }
    }
}

#[test]
fn one_click_turns_every_word_korean() {
    let mut harness = harness(app(scratch("toggle"), quiet_settings(), None), SIZES[0]);
    assert!(harness.query_all_by_label("Network").count() > 0);
    harness.get_by_label("한국어").click();
    harness.run_steps(2);
    assert!(harness.query_all_by_label("네트워크").count() > 0);
    assert_eq!(harness.query_all_by_label("Network").count(), 0);
    assert_eq!(harness.state_mut().settings_mut().language, Lang::Ko);
}

#[test]
fn the_first_run_offers_every_device() {
    let settings = Settings {
        setup_done: false,
        ..quiet_settings()
    };
    let harness = harness(app(scratch("first"), settings, None), SIZES[0]);
    for name in ["ESP32 boards", "Raspberry Pi", "Several nodes on this computer"] {
        assert!(harness.query_by_label(name).is_some(), "{name}");
    }
    assert!(harness.query_by_label("Computer and computer").is_some());
}

/// The program node processes run, built beside the tests.
fn node_program() -> PathBuf {
    let test = std::env::current_exe().unwrap();
    let name = format!("olc-core{}", std::env::consts::EXE_SUFFIX);
    let program = test.parent().unwrap().parent().unwrap().join(name);
    assert!(program.exists(), "build olc-core first");
    program
}

/// Long enough for a password's hash on a busy computer without optimisation.
#[track_caller]
fn wait_until(harness: &mut Harness<'static, App>, mut done: impl FnMut(&App) -> bool) {
    for _ in 0..600 {
        harness.run_steps(1);
        if done(harness.state()) {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("waited 60 s");
}

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

fn type_into(harness: &mut Harness<'static, App>, field: usize, text: &str) {
    let find = |harness: &Harness<'static, App>| {
        harness.query_all_by_role(Role::TextInput).nth(field).map(|node| node.focus())
    };
    find(harness);
    harness.run_steps(2);
    if let Some(node) = harness.query_all_by_role(Role::TextInput).nth(field) {
        node.type_text(text);
    }
    harness.run_steps(2);
}

/// The same for a field that hides what is typed.
fn type_secret(harness: &mut Harness<'static, App>, field: usize, text: &str) {
    if let Some(node) = harness.query_all_by_role(Role::PasswordInput).nth(field) {
        node.focus();
    }
    harness.run_steps(2);
    if let Some(node) = harness.query_all_by_role(Role::PasswordInput).nth(field) {
        node.type_text(text);
    }
    harness.run_steps(2);
}

fn row_of(snap: &Snapshot, name: &str) -> Option<Entry> {
    snap.groups
        .iter()
        .flat_map(|group| &group.chains)
        .flat_map(|chain| &chain.entries)
        .find(|entry| entry.name == name)
        .cloned()
}

/// Choosing several nodes on this computer starts them as real processes.
#[test]
fn the_first_run_starts_real_node_processes() {
    let settings = Settings {
        setup_done: false,
        port: 7310,
        ..quiet_settings()
    };
    let window = app(scratch("first-nodes"), settings, Some(node_program()));
    let mut harness = harness(window, SIZES[0]);
    harness.get_by_label("Several nodes on this computer").click();
    harness.run_steps(2);
    harness.get_by_label("Next").click();
    harness.run_steps(2);
    // A new coin starts easy, so its height moves while the table is watched.
    harness.get_by_label("New coin").click();
    harness.run_steps(2);
    type_into(&mut harness, 0, "Quick");
    type_into(&mut harness, 1, "QCK");
    harness.get_by_label("Start this coin").click();
    harness.run_steps(2);
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
    let settings = Settings {
        port: 7330,
        ..quiet_settings()
    };
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
    if let Some(folder) = std::env::var_os("OLC_SHOTS").map(PathBuf::from) {
        harness.state_mut().open("page.local");
        shot(&mut harness, &folder, "scene-kinds");
    }

    // The same choice on this app's own node, which comes back as the other kind.
    harness.set_size(egui::vec2(1280.0, 1900.0));
    harness.state_mut().open("page.settings");
    harness.run_steps(SETTLE_STEPS);
    harness.get_by_label("Keep headers only (light)").click();
    wait_until(&mut harness, |app| {
        let node = &app.snapshot().node;
        node.height >= 5 && node.light.is_some_and(|light| light.held_headers >= 5)
    });
    if let Some(folder) = std::env::var_os("OLC_SHOTS").map(PathBuf::from) {
        shot(&mut harness, &folder, "scene-own-light");
        harness.state_mut().open("page.keys");
        shot(&mut harness, &folder, "scene-keys-watch");
    }
    harness.state_mut().open("page.mining");
    harness.run_steps(SETTLE_STEPS);
    harness.get_by_label("A node that keeps only headers cannot mine.");
    harness.state_mut().open("page.explorer");
    harness.run_steps(SETTLE_STEPS);
    harness.get_by_label("This node keeps only headers, so it has no blocks to show.");
}

/// The owner's scene: a note goes into a block and shows in the explorer, and a second key is made,
/// backed up and restored, all by clicking in the window on a real node.
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
    settings.port = 7330;
    let window = app(root.clone(), settings, Some(node_program()));
    let mut harness = harness(window, SIZES[0]);
    harness.state_mut().settings_mut().language = Lang::En;

    harness.state_mut().open("page.keys");
    wait_until(&mut harness, |app| app.snapshot().node.keys.len() == 1);
    harness.run_steps(2);
    harness.get_by_label("Make a new key").click();
    wait_until(&mut harness, |app| app.snapshot().node.keys.len() == 2);
    harness.run_steps(2);
    harness.get_by_label("Receive here").click();
    wait_until(&mut harness, |app| app.snapshot().node.keys[1].receiving);

    let backup = root.join("wallet.backup");
    harness.set_size(egui::vec2(1280.0, 1400.0));
    harness.run_steps(SETTLE_STEPS);
    // Export path, watched address, then backup path. The password and the import are hidden.
    type_into(&mut harness, 2, backup.to_str().unwrap());
    type_secret(&mut harness, 1, "a long password");
    harness.get_by_label("Back up").click();
    wait_until(&mut harness, |app| {
        let failed = app.snapshot().notice.as_ref().is_some_and(|notice| !notice.good);
        failed || backup.exists()
    });
    let said = harness.state().snapshot().notice.clone().map(|notice| notice.detail);
    assert!(backup.exists(), "no backup: {said:?}");
    type_secret(&mut harness, 1, "a long password");
    harness.get_by_label("Restore").click();
    wait_until(&mut harness, |app| {
        app.snapshot().notice.as_ref().is_some_and(|notice| notice.key == "notice.restored")
    });
    let file = std::fs::read_to_string(&backup).unwrap();
    assert!(!file.contains("\"keys\""), "a password hides the keys");

    harness.state_mut().open("page.explorer");
    wait_until(&mut harness, |app| app.snapshot().node.height > 2);
    harness.run_steps(2);
    type_into(&mut harness, 1, "written by hand");
    harness.get_by_label("Send").click();
    wait_until(&mut harness, |app| {
        app.snapshot().node.notes.iter().any(|note| note.text == "written by hand")
    });
    harness.run_steps(SETTLE_STEPS);
    assert!(harness.query_all_by_label_contains("written by hand").next().is_some());
    if let Some(folder) = std::env::var_os("OLC_SHOTS").map(PathBuf::from) {
        shot(&mut harness, &folder, "scene-explorer");
    }
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
    harness.state_mut().open("page.network");
    wait_until(&mut harness, |app| app.snapshot().node.needs_history.is_some());
    assert_eq!(harness.state().snapshot().node.height, 0, "nothing moves without a yes");
    harness.run_steps(SETTLE_STEPS);
    if let Some(folder) = std::env::var_os("OLC_SHOTS").map(PathBuf::from) {
        shot(&mut harness, &folder, "scene-history");
    }
    harness.get_by_label("Trust this peer").click();
    wait_until(&mut harness, |app| app.snapshot().node.height >= tip);
    assert!(harness.state().snapshot().node.assumed_from.is_some());
    harness.run_steps(SETTLE_STEPS);
    if let Some(folder) = std::env::var_os("OLC_SHOTS").map(PathBuf::from) {
        shot(&mut harness, &folder, "scene-history-after");
    }
}

/// A coin running on this computer is found, and picking it needs no file.
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

    let settings = Settings {
        setup_done: false,
        port: 7320,
        ..quiet_settings()
    };
    let mut harness = harness(app(scratch("join"), settings, None), SIZES[0]);
    harness.get_by_label("Next").click();
    wait_until(&mut harness, |app| {
        app.snapshot().groups.iter().any(|group| {
            group.identity.as_ref().is_some_and(|identity| identity.coin.name == "Found")
        })
    });
    harness.run_steps(2);
    harness.get_by_label("Found").click();
    harness.run_steps(2);
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

fn shot(harness: &mut Harness<'static, App>, folder: &Path, name: &str) {
    harness.run_steps(SETTLE_STEPS);
    let image = harness.render().expect("a renderer");
    image.save(folder.join(format!("{name}.png"))).expect("save");
}

/// Writes the pictures when `OLC_SHOTS` names a folder. It needs `olc-core` built beside the test
/// program and a graphics adapter, which `offscreen.sh` provides.
#[test]
fn screenshots() {
    let Some(folder) = std::env::var_os("OLC_SHOTS").map(PathBuf::from) else {
        return;
    };
    std::fs::create_dir_all(&folder).unwrap();
    let program = node_program();
    let root = scratch("shots");
    let mut coin = CoinConfig::olc();
    coin.name = "Shelf".into();
    coin.ticker = "SHF".into();
    coin.message = "Shelf genesis".into();
    coin.initial_difficulty = 30_000;
    save_coin(&Paths { root: root.clone() }, &coin).unwrap();
    let mut settings = quiet_settings();
    settings.coin = Some(coin_folder(&coin));
    settings.device = Device::ThisComputer;
    settings.mine_on_start = true;
    settings.nodes = (0..3).map(|id| Preset::Esp32S3.spec(id)).collect();
    let window = app(root, settings, Some(program));
    let mut live = harness(window, SIZES[0]);
    let wait = std::env::var("OLC_SHOTS_WAIT").ok().and_then(|text| text.parse().ok());
    std::thread::sleep(Duration::from_secs(wait.unwrap_or(45)));
    let titles = live.state().page_titles();
    for theme in [Theme::Dark, Theme::Light] {
        for language in [Lang::En, Lang::Ko] {
            live.state_mut().settings_mut().theme = theme;
            live.state_mut().settings_mut().language = language;
            for (size, tag) in SIZES.iter().zip(["1280", "1920"]) {
                live.set_size(egui::vec2(size.0 as f32, size.1 as f32));
                for title in &titles {
                    live.state_mut().open(title);
                    let name = format!(
                        "{}-{language:?}-{theme:?}-{tag}",
                        title.trim_start_matches("page."),
                    );
                    shot(&mut live, &folder, &name.to_lowercase());
                }
            }
        }
    }
    live.state_mut().settings_mut().theme = Theme::Dark;
    live.state_mut().settings_mut().language = Lang::En;
    live.set_size(egui::vec2(1280.0, 800.0));
    live.state_mut().open("page.boards");
    live.run_steps(SETTLE_STEPS);
    if let Some(manage) = live.query_all_by_label("Manage").next() {
        manage.click();
    }
    shot(&mut live, &folder, "boards-manage");
    live.state_mut().open("page.local");
    live.run_steps(SETTLE_STEPS);
    live.get_by_label("New node").click();
    shot(&mut live, &folder, "local-new-node");
    let settings = Settings {
        setup_done: false,
        ..quiet_settings()
    };
    let mut first = harness(app(scratch("shots-first"), settings, None), SIZES[0]);
    shot(&mut first, &folder, "setup-device");
    first.get_by_label("Next").click();
    shot(&mut first, &folder, "setup-coin");
}
