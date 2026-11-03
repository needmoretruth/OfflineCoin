//! That what a screen says can be read: amounts alike on every page, and a board's settings as
//! the board said them.
use super::shots::{tap, with_boards};
use super::{app, harness, quiet_settings, scratch, type_into, wait_until, Window, SETTLE_STEPS, SIZES};
use crate::core::{Activity, ActivityKind, Snapshot, WorkerRow};
use crate::fleet::BoardState;
use crate::i18n::{self, Lang, Mode, Voice};
use egui::accesskit::{Role, Toggled};
use egui_kittest::kittest::{AccessKitNode, By, NodeT, Queryable};
use std::path::Path;

const COIN: u64 = 100_000_000;

/// A stage where every amount is large enough to need its commas.
fn large(quiet: &Snapshot) -> Snapshot {
    let mut stage = with_boards(quiet).unwrap_or_else(|| quiet.clone());
    stage.node.balance = 3_634 * COIN + COIN / 2;
    stage.node.spendable = 2_400 * COIN;
    stage.node.maturing = 1_234 * COIN + COIN / 2;
    let row = |index: u8| Activity {
        kind: ActivityKind::Mined,
        height: Some(u64::from(index)),
        amount: 7_250 * COIN,
        time_ms: 1,
        id: [index; 32],
        fee: None,
        size: 0,
        coins_in: 0,
        ripe_at: None,
    };
    stage.node.activity = (1..4).map(row).collect();
    stage.node.watched = vec![("olc11".into(), 9_100 * COIN)];
    for board in &mut stage.boards {
        board.seen.beacon.balance = 5_500 * COIN;
    }
    stage.pool.running = true;
    let paid = 8_800 * COIN;
    stage.pool.workers = vec![WorkerRow { worker: "Kitchen".into(), accepted: 1, paid, online: true, ..WorkerRow::default() }];
    stage
}

/// What the window says, field contents left out: those are what a person types.
fn said(window: &Window) -> Vec<String> {
    let field = |role: Role| matches!(role, Role::TextInput | Role::PasswordInput | Role::SpinButton);
    let inside = move |node: &AccessKitNode<'_>| node.parent().is_some_and(|parent| field(parent.role()));
    window
        .query_all(By::new().predicate(move |node| !field(node.role()) && !inside(node)))
        .filter_map(|node| node.accesskit_node().label().or_else(|| node.accesskit_node().value()))
        .collect()
}

/// Numbers of four digits or more with no comma. A number glued to letters or a colon is part of
/// a name or an address, and one beside a dash is a date.
fn ungrouped(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let joined = |c: Option<&char>| c.is_some_and(|c| c.is_ascii_alphanumeric() || matches!(c, ':' | '-'));
    let mut found = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        let start = index;
        while chars.get(index).is_some_and(|c| c.is_ascii_digit() || matches!(c, ',' | '.')) {
            index += 1;
        }
        if start == index {
            index += 1;
            continue;
        }
        let before = start.checked_sub(1).and_then(|at| chars.get(at));
        let run: String = chars[start..index].iter().collect();
        let run = run.trim_end_matches(['.', ',']);
        let after = chars.get(start + run.chars().count());
        let whole = run.split('.').next().unwrap_or_default();
        if !joined(before) && !joined(after) && whole.len() >= 4 && !whole.contains(',') {
            found.push(run.to_string());
        }
    }
    found
}

#[test]
fn the_check_tells_a_bare_amount_from_a_name() {
    assert_eq!(ungrouped("2400 SHF"), ["2400"]);
    assert_eq!(ungrouped("모두 1234.5개"), ["1234.5"]);
    assert_eq!(ungrouped("2,400 SHF, 20,159 and 999."), Vec::<String>::new());
    assert_eq!(ungrouped("olc12345 127.0.0.1:8333 2026-11-02"), Vec::<String>::new());
}

fn look(window: &Window, place: &str, wrong: &mut Vec<String>) {
    for text in said(window) {
        for number in ungrouped(&text) {
            wrong.push(format!("{place}: {number} in {text:?}"));
        }
    }
}

/// Walks every page and tab in both modes and languages.
fn every_view(window: &mut Window, look: &mut dyn FnMut(&Window, &str)) {
    for mode in [Mode::Basic, Mode::Expert] {
        for lang in [Lang::En, Lang::Ko] {
            window.state_mut().settings_mut().mode = mode;
            window.state_mut().settings_mut().language = lang;
            window.run_steps(SETTLE_STEPS);
            for title in window.state().page_titles() {
                window.state_mut().open(title);
                window.run_steps(SETTLE_STEPS);
                look(window, &format!("{mode:?} {lang:?} {title}"));
            }
            window.state_mut().open("page.boards");
            window.run_steps(SETTLE_STEPS);
            for tab in ["boards.tab_nodes", "boards.tab_pool"] {
                tap(window, tab);
                window.run_steps(SETTLE_STEPS);
                look(window, &format!("{mode:?} {lang:?} {tab}"));
            }
        }
    }
}

/// A screen reader says a field's name, and a field without one is only "edit".
#[test]
fn every_field_has_a_name() {
    let mut window = harness(app(scratch("field-names"), quiet_settings(), None), SIZES[1]);
    wait_until(&mut window, |app| !app.snapshot().node.address.is_empty());
    let stage = large(&window.state().snapshot());
    window.state_mut().set_stage(Some(stage));
    let mut wrong = Vec::new();
    let mut fields = 0;
    every_view(&mut window, &mut |window, place| {
        let field = |role| matches!(role, Role::TextInput | Role::PasswordInput | Role::SpinButton);
        for node in window.query_all(By::new().predicate(move |node| field(node.role()))) {
            fields += 1;
            let access = node.accesskit_node();
            if access.label().is_none_or(|name| name.trim().is_empty()) {
                wrong.push(format!("{place}: {:?}", access.value()));
            }
        }
    });
    assert!(fields > 8, "the walk found {fields} fields");
    assert!(wrong.is_empty(), "fields with no name: {wrong:#?}");
}

#[test]
fn every_amount_reads_with_its_commas() {
    let mut window = harness(app(scratch("readings"), quiet_settings(), None), SIZES[1]);
    wait_until(&mut window, |app| !app.snapshot().node.address.is_empty());
    let stage = large(&window.state().snapshot());
    window.state_mut().set_stage(Some(stage));
    let mut wrong = Vec::new();
    every_view(&mut window, &mut |window, place| look(window, place, &mut wrong));
    window.state_mut().settings_mut().language = Lang::En;
    window.state_mut().open("page.home");
    window.run_steps(SETTLE_STEPS);
    let home = said(&window);
    assert!(home.iter().any(|text| text.contains("3,634.5")), "the stage did not reach the screen: {home:#?}");
    assert!(wrong.is_empty(), "amounts without commas: {wrong:#?}");
}

#[test]
fn all_of_a_large_balance_fills_the_field_and_reaches_the_slip() {
    let mut window = harness(app(scratch("slip-amount"), quiet_settings(), None), SIZES[1]);
    wait_until(&mut window, |app| !app.snapshot().node.address.is_empty());
    let stage = large(&window.state().snapshot());
    let address = stage.node.address.clone();
    window.state_mut().set_stage(Some(stage));
    window.run_steps(SETTLE_STEPS);
    tap(&mut window, "home.send");
    type_into(&mut window, 0, &address);
    tap(&mut window, "send.all");
    window.run_steps(SETTLE_STEPS);
    tap(&mut window, "send.next");
    window.run_steps(SETTLE_STEPS);
    let slip = said(&window);
    assert!(slip.iter().any(|text| text.starts_with("2,400 ")), "no grouped amount on the slip: {slip:#?}");
}

#[test]
fn no_page_makes_an_amount_of_its_own() {
    fn walk(folder: &Path, wrong: &mut Vec<String>) {
        for entry in std::fs::read_dir(folder).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, wrong);
            } else if path.ends_with("ui/format.rs") || path.extension().is_none_or(|kind| kind != "rs") {
                continue;
            } else if std::fs::read_to_string(&path).is_ok_and(|text| text.contains(concat!("format", "_coins"))) {
                wrong.push(path.display().to_string());
            }
        }
    }
    let mut wrong = Vec::new();
    walk(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut wrong);
    assert!(wrong.is_empty(), "amounts go through ui::format: {wrong:?}");
}

fn switch_of(window: &Window, key: &str) -> Option<bool> {
    let settings = window.state().settings();
    let name = i18n::text(Voice { lang: settings.language, mode: settings.mode }, key);
    let mut found = window.query_all(By::new().role(Role::Switch).label(name));
    found.next().map(|node| matches!(node.accesskit_node().toggled(), Some(Toggled::True)))
}

#[test]
fn a_board_shows_what_it_said_and_two_words_only_when_it_said_nothing() {
    let mut window = harness(app(scratch("board-state"), quiet_settings(), None), SIZES[1]);
    wait_until(&mut window, |app| !app.snapshot().node.address.is_empty());
    let quiet = window.state().snapshot();
    let mut stage = with_boards(&quiet).unwrap_or_else(|| (*quiet).clone());
    stage.boards.truncate(1);
    let state = BoardState { nearby: Some(true), bluetooth: Some(false), own_network: Some(true), ..BoardState::default() };
    stage.boards[0].state = state;
    window.state_mut().set_stage(Some(stage.clone()));
    window.state_mut().open("page.boards");
    window.run_steps(SETTLE_STEPS);
    assert_eq!(switch_of(&window, "boards.nearby"), Some(true));
    assert_eq!(switch_of(&window, "boards.bluetooth"), Some(false));
    assert_eq!(switch_of(&window, "boards.wifi"), Some(true));

    stage.boards[0].state = BoardState { own_network: Some(false), ..BoardState::default() };
    window.state_mut().set_stage(Some(stage));
    window.run_steps(SETTLE_STEPS);
    assert_eq!(switch_of(&window, "boards.nearby"), None, "older firmware says nothing, so no switch");
    assert_eq!(switch_of(&window, "boards.wifi"), Some(false));
    let voice = Voice { lang: Lang::En, mode: Mode::Basic };
    assert_eq!(window.query_all_by_label(i18n::text(voice, "common.on")).count(), 2);
}
