//! Writes the pictures when `OLC_SHOTS` names a folder: every state, at both window sizes, in both
//! themes, languages and modes, with text at normal and 1.5 times. `OLC_SHOTS_ONLY` keeps the
//! views whose names hold that word, and `OLC_SHOTS_BRIEF` draws each once in each language.
//! Nodes, coins and blocks are real. Boards are staged, since no test may touch a port.
use super::{
    app, click, harness, node_program, port_of_scene, quiet_settings, scratch, staged_board, type_into, wait_until,
    Window, SETTLE_STEPS, SIZES,
};
use crate::core::{coin_folder, save_coin, Notice, Snapshot, WorkerRow};
use crate::i18n::{self, Lang, Mode, Voice};
use crate::local::{Preset, Supervisor};
use crate::settings::{Device, Paths, Settings, Theme};
use egui_kittest::kittest::Queryable;
use olc_core::CoinConfig;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

const BIG_TEXT: u16 = 150;
/// Blocks and coins take a little while to appear on a real node.
const LIVE_SECONDS: u64 = 40;

pub(super) fn shot(harness: &mut Window, folder: &Path, name: &str) {
    harness.run_steps(SETTLE_STEPS);
    let image = harness.render().expect("a renderer");
    // Made here, so a test that saves first does not hang on another making the folder.
    std::fs::create_dir_all(folder).expect("the picture folder");
    image.save(folder.join(format!("{name}.png"))).expect("save");
}

fn modes(expert_only: bool) -> &'static [Mode] {
    if expert_only {
        &[Mode::Expert]
    } else {
        &[Mode::Basic, Mode::Expert]
    }
}

fn looks(view: &str, window: &mut Window, folder: &Path, mode: Mode, language: Lang) {
    let brief = std::env::var_os("OLC_SHOTS_BRIEF").is_some();
    let themes: &[Theme] = if brief { &[Theme::Dark] } else { &[Theme::Dark, Theme::Light] };
    let sizes: &[(u32, u32)] = if brief { &SIZES[..1] } else { &SIZES };
    let texts: &[u16] = if brief { &[100] } else { &[100, BIG_TEXT] };
    for &theme in themes {
        for &size in sizes {
            for &text in texts {
                let settings = window.state_mut().settings_mut();
                settings.theme = theme;
                settings.text_percent = text;
                // The size is read through the zoom, so the zoom has to land first.
                window.run_steps(2);
                window.set_size(egui::vec2(size.0 as f32, size.1 as f32));
                let name = format!("{view}-{mode:?}-{language:?}-{theme:?}-{}-{text}", size.0);
                shot(window, folder, &name.to_lowercase());
            }
        }
    }
}

fn speak(window: &mut Window, mode: Mode, language: Lang) {
    let settings = window.state_mut().settings_mut();
    settings.mode = mode;
    settings.language = language;
}

fn back_to_normal(window: &mut Window) {
    speak(window, Mode::Basic, Lang::En);
    let settings = window.state_mut().settings_mut();
    settings.theme = Theme::Dark;
    settings.text_percent = 100;
    window.run_steps(2);
    window.set_size(egui::vec2(SIZES[0].0 as f32, SIZES[0].1 as f32));
    window.run_steps(SETTLE_STEPS);
}

fn all_combinations(view: &str, window: &mut Window, folder: &Path) {
    for &mode in modes(false) {
        for language in [Lang::En, Lang::Ko] {
            speak(window, mode, language);
            looks(view, window, folder, mode, language);
        }
    }
    back_to_normal(window);
}

struct View {
    name: &'static str,
    page: &'static str,
    /// What the window holds in place of the core's view of boards and notices.
    stage: fn(&Snapshot) -> Option<Snapshot>,
    /// The clicks that lead to this state, in the words of the mode and language on screen.
    go: fn(&mut Window),
    expert_only: bool,
}

fn real(_: &Snapshot) -> Option<Snapshot> {
    None
}

fn nothing(_: &mut Window) {}

pub(super) fn with_boards(live: &Snapshot) -> Option<Snapshot> {
    let mut hallway = staged_board(true, false);
    hallway.seen.id = [5; 6];
    hallway.seen.beacon.name = "Hallway".into();
    hallway.seen.beacon.hashrate = 0;
    hallway.seen.beacon.mining = false;
    let mut shed = staged_board(false, false);
    shed.seen.id = [6; 6];
    shed.seen.beacon.name = "Shed".into();
    shed.job = Some("update_failed".into());
    let boards = vec![staged_board(true, false), hallway, shed];
    Some(Snapshot { boards, new_ports: vec![("ttyACM0".into(), None)], usb: true, ..live.clone() })
}

fn with_a_stray_board(live: &Snapshot) -> Option<Snapshot> {
    let mut snap = with_boards(live)?;
    if let Some(board) = snap.boards.first_mut() {
        board.seen.beacon.genesis = [7; 32];
    }
    Some(snap)
}

fn no_boards(live: &Snapshot) -> Option<Snapshot> {
    Some(Snapshot { usb: true, ..live.clone() })
}

fn with_failure(live: &Snapshot) -> Option<Snapshot> {
    let notice = Notice { number: 900, key: "notice.not_enough", detail: String::new(), good: false };
    Some(Snapshot { notice: Some(notice), ..live.clone() })
}

fn with_pool(live: &Snapshot) -> Option<Snapshot> {
    let row = |name: &str, accepted, paid| WorkerRow {
        name: format!("olc1example.{name}"),
        worker: name.into(),
        speed: 40_000,
        last_share_secs: Some(3),
        accepted,
        paid,
        difficulty: 500,
        online: true,
        ..WorkerRow::default()
    };
    let mut stage = with_boards(live)?;
    stage.pool.running = true;
    stage.pool.port = 3333;
    stage.pool.reach = vec!["127.0.0.1:3333".into()];
    stage.pool.workers = vec![row("Kitchen", 1_204, 620_000_000), row("Hallway", 880, 410_000_000)];
    Some(stage)
}

const EMPTY: &[View] = &[
    View { name: "home-empty", page: "page.home", stage: real, go: nothing, expert_only: false },
    View { name: "boards-empty", page: "page.boards", stage: no_boards, go: nothing, expert_only: false },
    View { name: "coins-empty", page: "page.coins", stage: real, go: nothing, expert_only: false },
    View { name: "records-empty", page: "page.records", stage: real, go: nothing, expert_only: false },
    View { name: "settings", page: "page.settings", stage: real, go: nothing, expert_only: false },
    View { name: "nodes-empty", page: "page.boards", stage: real, go: |w| tap(w, "boards.tab_nodes"), expert_only: false },
    View { name: "pool-empty", page: "page.boards", stage: real, go: |w| tap(w, "boards.tab_pool"), expert_only: false },
    View { name: "console", page: "page.console", stage: real, go: nothing, expert_only: true },
];

const LIVE: &[View] = &[
    View { name: "home-working", page: "page.home", stage: real, go: nothing, expert_only: false },
    View { name: "home-error", page: "page.home", stage: with_failure, go: nothing, expert_only: false },
    View { name: "home-receive", page: "page.home", stage: real, go: |w| tap(w, "home.receive"), expert_only: false },
    View { name: "home-send", page: "page.home", stage: real, go: |w| tap(w, "home.send"), expert_only: false },
    View { name: "home-send-wrong", page: "page.home", stage: real, go: send_wrong, expert_only: false },
    View { name: "home-send-slip", page: "page.home", stage: real, go: send_right, expert_only: false },
    View { name: "boards-working", page: "page.boards", stage: with_boards, go: nothing, expert_only: false },
    View { name: "boards-move-slip", page: "page.boards", stage: with_a_stray_board, go: |w| tap(w, "boards.move"), expert_only: false },
    View { name: "nodes-working", page: "page.boards", stage: real, go: |w| tap(w, "boards.tab_nodes"), expert_only: false },
    View { name: "nodes-remove-slip", page: "page.boards", stage: real, go: remove_slip, expert_only: false },
    View { name: "pool-working", page: "page.boards", stage: with_pool, go: |w| tap(w, "boards.tab_pool"), expert_only: false },
    View { name: "coins-working", page: "page.coins", stage: real, go: nothing, expert_only: false },
    View { name: "coins-new", page: "page.coins", stage: real, go: |w| tap(w, "coins.new"), expert_only: false },
    View { name: "records-working", page: "page.records", stage: real, go: nothing, expert_only: false },
    View { name: "records-block", page: "page.records", stage: real, go: open_block, expert_only: false },
    View { name: "records-note-slip", page: "page.records", stage: real, go: note_slip, expert_only: false },
    View { name: "settings-backup", page: "page.settings", stage: real, go: |w| tap(w, "settings.backup_go"), expert_only: false },
    View { name: "settings-picker", page: "page.settings", stage: real, go: picker, expert_only: false },
    View { name: "settings-addresses", page: "page.settings", stage: real, go: addresses, expert_only: false },
    View { name: "settings-offline", page: "page.settings", stage: real, go: offline, expert_only: false },
];

fn word(window: &Window, key: &str) -> String {
    let settings = window.state().settings();
    i18n::text(Voice { lang: settings.language, mode: settings.mode }, key).to_string()
}

/// Presses the named control wherever it sits, also below the edge of the window.
pub(super) fn tap(window: &mut Window, key: &str) {
    let name = word(window, key);
    let Some(control) = window.query_all_by_label(&name).next() else {
        panic!("{key} is not on the screen");
    };
    control.click_accesskit();
    window.run_steps(2);
}

fn tap_nth(window: &mut Window, key: &str, index: usize) {
    let name = word(window, key);
    if let Some(control) = window.query_all_by_label(&name).nth(index) {
        control.click_accesskit();
    }
    window.run_steps(SETTLE_STEPS);
}

fn send_wrong(window: &mut Window) {
    tap(window, "home.send");
    type_into(window, 0, "not an address");
    type_into(window, 1, "1");
    tap(window, "send.next");
}

fn send_right(window: &mut Window) {
    let address = window.state().snapshot().node.address.clone();
    tap(window, "home.send");
    type_into(window, 0, &address);
    type_into(window, 1, "1");
    tap(window, "send.next");
}

fn remove_slip(window: &mut Window) {
    tap(window, "boards.tab_nodes");
    // Only a stopped node can go.
    if window.state().snapshot().local.iter().all(|view| view.pid.is_some()) {
        tap(window, "local.stop");
        wait_until(window, |app| app.snapshot().local.iter().any(|view| view.pid.is_none()));
        window.run_steps(SETTLE_STEPS);
    }
    tap(window, "local.remove");
}

fn open_block(window: &mut Window) {
    tap_nth(window, "records.open", 0);
}

fn note_slip(window: &mut Window) {
    type_into(window, 1, "We were here");
    tap(window, "records.write_go");
}

fn picker(window: &mut Window) {
    tap(window, "settings.backup_go");
    tap(window, "backup.choose");
}

fn addresses(window: &mut Window) {
    tap_nth(window, "common.open", 0);
}

fn offline(window: &mut Window) {
    tap_nth(window, "common.open", 2);
}

fn wanted(view: &View) -> bool {
    std::env::var("OLC_SHOTS_ONLY").map_or(true, |only| only.split(',').any(|word| view.name.contains(word)))
}

fn draw_views(views: &[View], window: &mut Window, folder: &Path) {
    for view in views.iter().filter(|view| wanted(view)) {
        for &mode in modes(view.expert_only) {
            for language in [Lang::En, Lang::Ko] {
                let live = window.state().snapshot();
                let stage = (view.stage)(&live);
                window.state_mut().set_stage(stage);
                speak(window, mode, language);
                window.state_mut().open(view.page);
                window.run_steps(SETTLE_STEPS);
                (view.go)(window);
                looks(view.name, window, folder, mode, language);
                window.key_press(egui::Key::Escape);
                window.run_steps(2);
                window.key_press(egui::Key::Escape);
                window.run_steps(2);
                window.state_mut().set_stage(None);
            }
        }
    }
    back_to_normal(window);
}

fn setup_shots(folder: &Path) {
    let settings = Settings { setup_done: false, port: port_of_scene(72), ..quiet_settings() };
    let mut first = harness(app(scratch("shots-first"), settings, None), SIZES[0]);
    all_combinations("setup-device", &mut first, folder);
    click(&mut first, "ESP32 boards");
    all_combinations("setup-coin", &mut first, folder);
    click(&mut first, "New coin");
    all_combinations("setup-new-coin", &mut first, folder);
    click(&mut first, "Next");
    all_combinations("setup-mine", &mut first, folder);
}

/// It needs `olc-core` built beside the test program and a graphics adapter, which `offscreen.sh`
/// provides.
#[test]
fn screenshots() {
    let Some(folder) = std::env::var_os("OLC_SHOTS").map(std::path::PathBuf::from) else {
        return;
    };
    std::fs::create_dir_all(&folder).unwrap();
    let mut empty = harness(app(scratch("shots-empty"), Settings { port: port_of_scene(74), ..quiet_settings() }, None), SIZES[0]);
    wait_until(&mut empty, |app| !app.snapshot().node.address.is_empty());
    draw_views(EMPTY, &mut empty, &folder);
    drop(empty);
    setup_shots(&folder);

    let root = scratch("shots");
    let paths = Paths { root: root.clone() };
    let mut coin = CoinConfig::olc();
    coin.name = "Shelf".into();
    coin.ticker = "SHF".into();
    coin.message = "Shelf genesis".into();
    coin.initial_difficulty = 30_000;
    coin.rules.block_time_ms = 2_000;
    save_coin(&paths, &coin).unwrap();
    let mut neighbour = coin.clone();
    neighbour.name = "Found".into();
    neighbour.ticker = "FND".into();
    neighbour.message = "Found genesis".into();
    let file = save_coin(&paths, &neighbour).unwrap();
    let mut others = Supervisor::with_program(paths, node_program());
    others.start(&Preset::Esp32S3.spec(9), &file, &[]).unwrap();
    let mut settings = quiet_settings();
    settings.coin = Some(coin_folder(&coin));
    settings.device = Device::SeveralNodes;
    settings.mine_on_start = true;
    settings.scan_lan = true;
    settings.port = port_of_scene(76);
    settings.nodes = (0..2).map(|id| Preset::Esp32S3.spec(id)).collect();
    let mut live = harness(app(root, settings, Some(node_program())), SIZES[0]);
    let seconds = std::env::var("OLC_SHOTS_WAIT").ok().and_then(|text| text.parse().ok());
    std::thread::sleep(Duration::from_secs(seconds.unwrap_or(LIVE_SECONDS)));
    live.run_steps(SETTLE_STEPS);
    let _: Arc<Snapshot> = live.state().snapshot();
    draw_views(LIVE, &mut live, &folder);
    others.stop_all();
}
