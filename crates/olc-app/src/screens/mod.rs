//! Tests of the whole window: every page in every language and mode, drawn by the real app on a
//! real core. Node processes in the scenes are real. No test may open a serial port, so boards
//! appear only in a staged view, which `shots` and `boards_show_what_they_do` build here.
mod readings;
mod scenes;
mod rule_shots;
mod shots;
mod steady;

use crate::app::App;
use crate::core::{BoardView, Core, CoreLaunch, Snapshot};
use crate::discovery::Reach;
use crate::fleet::{BoardState, Connection, Seen};
use crate::i18n::{Lang, Mode};
use crate::settings::{Paths, Settings};
use egui::accesskit::Role;
use egui_kittest::kittest::{By, Queryable};
use egui_kittest::Harness;
use olc_control::Beacon;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub(super) const SIZES: [(u32, u32); 2] = [(1280, 800), (1920, 1080)];
pub(super) const SETTLE_STEPS: usize = 4;

pub(super) type Window = Harness<'static, App>;

pub(super) fn scratch(name: &str) -> PathBuf {
    let folder = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../.scratch/tests")
        .join(name);
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).unwrap();
    folder
}

pub(super) fn quiet_settings() -> Settings {
    Settings {
        setup_done: true,
        scan_lan: false,
        broadcast: false,
        port: port_of_scene(0),
        ..Settings::default()
    }
}

pub(super) fn app(root: PathBuf, settings: Settings, program: Option<PathBuf>) -> App {
    let launch = CoreLaunch {
        paths: Paths { root: root.clone() },
        fleet: None,
        usb: false,
        node_program: program,
    };
    let core = Core::start(launch, settings.clone()).unwrap();
    let mut app = App::new(core, settings, None);
    app.set_folder(root);
    app
}

pub(super) fn harness(app: App, size: (u32, u32)) -> Window {
    let draw = |ui: &mut egui::Ui, app: &mut App| {
        // The harness leaves a margin that the real window does not have.
        let whole = ui.ctx().content_rect();
        ui.scope_builder(egui::UiBuilder::new().max_rect(whole), |ui| {
            ui.set_clip_rect(whole);
            app.draw(ui);
        });
    };
    let mut harness = Harness::builder()
        .with_size(egui::vec2(size.0 as f32, size.1 as f32))
        .with_render_cursor(false)
        .build_ui_state(draw, app);
    harness.run_steps(SETTLE_STEPS);
    harness
}

/// The program node processes run, built beside the tests.
/// Another working copy may run these scenes at the same moment, so no scene owns a fixed port.
pub(super) fn port_of_scene(offset: u16) -> u16 {
    20_000 + (std::process::id() % 500) as u16 * 80 + offset
}

pub(super) fn node_program() -> PathBuf {
    let test = std::env::current_exe().unwrap();
    let name = format!("olc-core{}", std::env::consts::EXE_SUFFIX);
    let program = test.parent().unwrap().parent().unwrap().join(name);
    assert!(program.exists(), "build olc-core first");
    program
}

/// Long enough for a password's hash on a busy computer without optimisation.
#[track_caller]
pub(super) fn wait_until(harness: &mut Window, mut done: impl FnMut(&App) -> bool) {
    for _ in 0..600 {
        harness.run_steps(1);
        if done(harness.state()) {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let snapshot = harness.state().snapshot();
    let nodes: Vec<_> =
        snapshot.local.iter().map(|view| (&view.spec.name, view.pid, &view.ended, view.status.is_some())).collect();
    let own = (snapshot.node.height, snapshot.node.peers, snapshot.node.light);
    panic!("waited 60 s, last notice {:?}, nodes {nodes:?}, own {own:?}", snapshot.notice);
}

/// Waits for this notice, and fails at once on a notice that says something went wrong.
#[track_caller]
pub(super) fn wait_for_notice(harness: &mut Window, key: &str) {
    wait_until(harness, |app| {
        let snap = app.snapshot();
        let Some(notice) = &snap.notice else {
            return false;
        };
        assert!(notice.good, "{notice:?}");
        notice.key == key
    });
}

pub(super) fn click(harness: &mut Window, label: &str) {
    // A row that is a button also holds its text as a label of the same name.
    let button = harness.query(By::new().role(Role::Button).label(label));
    button.unwrap_or_else(|| harness.get_by_label(label)).click();
    harness.run_steps(2);
}

pub(super) fn type_into(harness: &mut Window, field: usize, text: &str) {
    if let Some(node) = harness.query_all_by_role(Role::TextInput).nth(field) {
        node.focus();
    }
    harness.run_steps(2);
    if let Some(node) = harness.query_all_by_role(Role::TextInput).nth(field) {
        node.type_text(text);
    }
    harness.run_steps(2);
}

/// The same for a field that hides what is typed.
pub(super) fn type_secret(harness: &mut Window, field: usize, text: &str) {
    if let Some(node) = harness.query_all_by_role(Role::PasswordInput).nth(field) {
        node.focus();
    }
    harness.run_steps(2);
    if let Some(node) = harness.query_all_by_role(Role::PasswordInput).nth(field) {
        node.type_text(text);
    }
    harness.run_steps(2);
}

const BOARD: [u8; 6] = [4; 6];

/// A board as the core reports one. Only tests build this: no test may touch a real port.
pub(super) fn staged_board(online: bool, returning: bool) -> BoardView {
    let beacon = Beacon {
        name: "Kitchen".into(),
        limit: 1_000,
        hashrate: 980,
        mining: online,
        height: 1_204,
        peers: 2,
        ..Beacon::default()
    };
    let seen = Seen {
        id: BOARD,
        beacon,
        connection: Connection::Direct { link: 0 },
        online,
        silent_for: Duration::ZERO,
        old_format: false,
    };
    let job = returning.then(|| "restarting".to_string());
    let state = BoardState {
        nearby: Some(true),
        bluetooth: Some(false),
        own_network: Some(true),
        on_wifi: Some(false),
        merging: Some(false),
        headers_only: Some(false),
        clock: Some(160),
        power: Some(8),
        keep: Some(0),
        ..BoardState::default()
    };
    BoardView { seen, job, answer: None, keeps: olc_node::Keeps::Everything, returning, reach: Reach::Usb, state }
}

pub(super) fn staged(boards: Vec<BoardView>) -> Snapshot {
    Snapshot { boards, usb: true, ..Snapshot::default() }
}

#[test]
fn every_page_draws_in_both_languages_and_modes() {
    let mut harness = harness(app(scratch("pages"), quiet_settings(), None), SIZES[0]);
    for mode in [Mode::Basic, Mode::Expert] {
        for language in [Lang::En, Lang::Ko] {
            harness.state_mut().settings_mut().language = language;
            harness.state_mut().settings_mut().mode = mode;
            for title in harness.state().page_titles() {
                harness.state_mut().open(title);
                harness.run_steps(2);
                assert_eq!(harness.query_all_by_label("?").count(), 0, "{title} {mode:?}");
            }
        }
    }
}

#[test]
fn basic_mode_has_five_places_and_expert_mode_adds_the_console() {
    let mut harness = harness(app(scratch("modes"), quiet_settings(), None), SIZES[0]);
    for place in ["Home", "Boards", "Coins", "Records", "Settings"] {
        assert!(harness.query_all_by_label(place).count() > 0, "{place}");
    }
    assert_eq!(harness.query_all_by_label("Console").count(), 0);
    harness.state_mut().open("page.settings");
    harness.run_steps(2);
    click(&mut harness, "Expert");
    assert_eq!(harness.state_mut().settings_mut().mode, Mode::Expert);
    harness.run_steps(2);
    for place in ["Wallet", "Devices", "Network", "Explorer", "Console"] {
        assert!(harness.query_all_by_label(place).count() > 0, "{place}");
    }
}

#[test]
fn one_click_turns_every_word_korean() {
    let mut harness = harness(app(scratch("toggle"), quiet_settings(), None), SIZES[0]);
    harness.state_mut().open("page.settings");
    harness.run_steps(2);
    assert!(harness.query_all_by_label("Coins").count() > 0);
    click(&mut harness, "한국어");
    assert!(harness.query_all_by_label("코인").count() > 0);
    assert_eq!(harness.query_all_by_label("Coins").count(), 0);
    assert_eq!(harness.state_mut().settings_mut().language, Lang::Ko);
}

#[test]
fn the_first_run_offers_every_device() {
    let settings = Settings { setup_done: false, ..quiet_settings() };
    let harness = harness(app(scratch("first"), settings, None), SIZES[0]);
    for name in ["Just this computer", "Several miners", "Two computers", "Raspberry Pi", "ESP32 boards"] {
        assert!(harness.query_by_label(name).is_some(), "{name}");
    }
}

/// Every page says the next step when it has nothing yet, with a way to take it.
#[test]
fn an_empty_home_says_what_to_do() {
    let harness = harness(app(scratch("empty"), quiet_settings(), None), SIZES[0]);
    assert!(harness.query_by_label("Balance").is_some());
    assert!(harness.query_all_by_label("Start mining").count() > 0);
    assert!(harness.query_by_label("Receive").is_some());
    assert!(harness.query_by_label("Send").is_some());
    assert!(harness.query_by_label("Start mining and your first coins appear here.").is_some());
}

/// A board that answers can be named and dimmed, and one coming back says so.
#[test]
fn boards_show_what_they_do() {
    let mut harness = harness(app(scratch("board"), quiet_settings(), None), SIZES[0]);
    harness.state_mut().set_stage(Some(staged(vec![staged_board(true, false)])));
    harness.state_mut().open("page.boards");
    harness.run_steps(SETTLE_STEPS);
    assert!(harness.query_all_by_label("Kitchen").count() > 0);
    assert!(harness.query_by_label("Dim").is_some());
    assert!(harness.query_all_by_label("Mining").count() > 0);
    harness.state_mut().set_stage(Some(staged(vec![staged_board(false, true)])));
    harness.run_steps(SETTLE_STEPS);
    assert!(harness.query_all_by_label("Restarting").count() > 0);
    assert!(harness.query_by_label("Dim").is_none());
}

#[test]
fn no_board_says_how_to_add_one() {
    let mut harness = harness(app(scratch("no-board"), quiet_settings(), None), SIZES[0]);
    harness.state_mut().set_stage(Some(staged(Vec::new())));
    harness.state_mut().open("page.boards");
    harness.run_steps(SETTLE_STEPS);
    let line = "Plug a board into this computer with a USB cable. It appears here by itself.";
    assert!(harness.query_by_label(line).is_some());
}

#[test]
fn receive_shows_a_code_and_the_address() {
    let mut harness = harness(app(scratch("receive"), quiet_settings(), None), SIZES[0]);
    wait_until(&mut harness, |app| !app.snapshot().node.address.is_empty());
    harness.run_steps(2);
    click(&mut harness, "Receive");
    assert!(harness.query_by_label("Code for your address").is_some());
    assert!(harness.query_by_label("Copy address").is_some());
}
