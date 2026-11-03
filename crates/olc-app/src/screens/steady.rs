//! A control stays the same widget while the data around it moves. egui names a widget by how many
//! came before it, so anything that appears above a control renames it, and a press that was on the
//! old name is lost, with the typing focus. That cost a click on a slow test runner, when a notice
//! bar came up between press and release. A person pays the same way.
use super::shots::with_boards;
use super::{app, harness, quiet_settings, scratch, wait_until, Window, SETTLE_STEPS, SIZES};
use crate::core::{Activity, ActivityKind, BlockRow, KeyView, NoteRow, Notice, PoolRow, Snapshot};
use crate::i18n::{self, Mode, Voice};
use egui::accesskit::Role;
use egui::{Event, Modifiers, PointerButton, Pos2};
use egui_kittest::kittest::{By, NodeT, Queryable};
use std::collections::BTreeMap;

fn press(window: &mut Window, label: &str) -> Pos2 {
    let pos = window.get_by_label(label).rect().center();
    window.event(Event::PointerMoved(pos));
    window.run_steps(1);
    let (button, modifiers) = (PointerButton::Primary, Modifiers::default());
    window.event(Event::PointerButton { pos, button, pressed: true, modifiers });
    window.run_steps(1);
    pos
}

fn release(window: &mut Window, pos: Pos2) {
    let (button, modifiers) = (PointerButton::Primary, Modifiers::default());
    window.event(Event::PointerButton { pos, button, pressed: false, modifiers });
    window.run_steps(1);
}

fn word(window: &Window, key: &str) -> String {
    let settings = window.state().settings();
    i18n::text(Voice { lang: settings.language, mode: settings.mode }, key).to_string()
}

#[test]
fn a_notice_between_press_and_release_leaves_the_click() {
    let mut window = harness(app(scratch("press"), quiet_settings(), None), SIZES[0]);
    let quiet = window.state().snapshot();
    window.state_mut().set_stage(Some((*quiet).clone()));
    window.run_steps(SETTLE_STEPS);
    let receive = word(&window, "home.receive");
    let pos = press(&mut window, &receive);
    let said = Notice { number: 7, key: "notice.saved", detail: String::new(), good: true };
    window.state_mut().set_stage(Some(Snapshot { notice: Some(said), ..(*quiet).clone() }));
    release(&mut window, pos);
    window.run_steps(2);
    let copy = word(&window, "receive.copy");
    assert!(window.query_by_label(&copy).is_some(), "the click was lost");
}

/// Everything that can turn up on its own while a person reads.
fn busy(quiet: &Snapshot) -> Snapshot {
    let mut stage = with_boards(quiet).unwrap_or_else(|| quiet.clone());
    stage.notice = Some(Notice { number: 8, key: "notice.saved", detail: String::new(), good: true });
    stage.node.mining = true;
    stage.node.hashrate = 20_000;
    stage.node.balance = 7_500_000_000;
    let row = |index: u8| Activity {
        kind: ActivityKind::Mined,
        height: Some(u64::from(index)),
        amount: 2_500_000_000,
        time_ms: 1,
        id: [index; 32],
        fee: None,
        size: 0,
        coins_in: 0,
        ripe_at: None,
    };
    stage.node.activity = (1..4).map(row).collect();
    stage.node.spendable = 2_500_000_000;
    stage.node.maturing = 5_000_000_000;
    stage.node.syncing_to = Some(90);
    stage.node.assumed_from = Some(10);
    stage.node.fetching = Some((1, 4));
    stage.node.rebuilt = true;
    stage.node.storage_full = true;
    stage.node.blocks = vec![BlockRow::default()];
    // Line for line: the first block may carry notes of its own.
    stage.node.notes = match quiet.node.notes.is_empty() {
        true => vec![NoteRow { height: 1, time_ms: 1, text: "note".into() }],
        false => quiet.node.notes.iter().map(|note| NoteRow { text: "note".into(), ..note.clone() }).collect(),
    };
    stage.node.watched = vec![("olc11".into(), 1)];
    stage.node.keys.push(KeyView { address: "olc22".into(), ..KeyView::default() });
    stage.scan.running = true;
    stage.scan.subnets = vec!["192.168.0.0/24".into()];
    stage.experiment.running = true;
    stage.experiment.lines = vec!["line".into()];
    stage.pool.running = true;
    stage.pool.rows = vec![PoolRow { name: "Kitchen".into(), shares: 1, rejected: 0, paid: 0 }];
    stage
}

/// Named controls on screen. A name used twice says nothing, so it counts for neither side.
fn controls(window: &Window) -> BTreeMap<String, Option<(String, f32)>> {
    let wanted = |role: Role| {
        matches!(role, Role::Button | Role::TextInput | Role::PasswordInput | Role::Switch | Role::Tab)
    };
    let mut found = BTreeMap::new();
    for node in window.query_all(By::new().predicate(move |node| wanted(node.role()))) {
        let access = node.accesskit_node();
        let Some(name) = access.label().or_else(|| access.value()) else {
            continue;
        };
        let place = (format!("{:?}", access.id()), node.rect().top());
        found.entry(name).and_modify(|place| *place = None).or_insert(Some(place));
    }
    found
}

fn compare(window: &mut Window, quiet: &Snapshot, moved: &Snapshot, place: &str, wrong: &mut Vec<String>) {
    window.state_mut().set_stage(Some(quiet.clone()));
    window.run_steps(SETTLE_STEPS);
    let before = controls(window);
    window.state_mut().set_stage(Some(moved.clone()));
    window.run_steps(SETTLE_STEPS);
    let after = controls(window);
    // The line after a repair is left out: it comes once, as the app starts.
    let mut calm = moved.clone();
    calm.node.rebuilt = false;
    window.state_mut().set_stage(Some(calm));
    window.run_steps(SETTLE_STEPS);
    let settled = controls(window);
    for (name, was) in &before {
        let Some((id, top)) = was else {
            continue;
        };
        if let Some(Some((same, _))) = after.get(name) {
            if id != same {
                wrong.push(format!("{place}: {name} became another widget"));
            }
        }
        if let Some(Some((_, now))) = settled.get(name) {
            if (top - now).abs() > 1.0 {
                wrong.push(format!("{place}: {name} moved from {top} to {now}"));
            }
        }
    }
}

#[test]
fn what_the_core_says_leaves_every_control_where_it_is() {
    let mut window = harness(app(scratch("steady"), quiet_settings(), None), SIZES[0]);
    wait_until(&mut window, |app| !app.snapshot().node.address.is_empty());
    let quiet = (*window.state().snapshot()).clone();
    let moved = busy(&quiet);
    let mut wrong = Vec::new();
    for mode in [Mode::Basic, Mode::Expert] {
        window.state_mut().settings_mut().mode = mode;
        for title in window.state().page_titles() {
            window.state_mut().open(title);
            compare(&mut window, &quiet, &moved, &format!("{mode:?} {title}"), &mut wrong);
        }
    }
    // A board on the list, and then a new one in the port.
    let boards = Snapshot { boards: moved.boards.clone(), ..quiet.clone() };
    window.state_mut().open("page.boards");
    compare(&mut window, &boards, &moved, "a new port", &mut wrong);
    assert!(wrong.is_empty(), "these did not hold still: {wrong:#?}");
}

/// What only a sheet shows: older coins that mature, so the line saying they must wait goes, or a
/// new mined coin that brings it back.
fn wallet_moves(quiet: &Snapshot, ripe: bool) -> Snapshot {
    let mut stage = quiet.clone();
    let wallet = &mut stage.node.wallet;
    wallet.has_words = true;
    wallet.words_to_write = None;
    wallet.outside_words = 5_000_000_000;
    wallet.outside_ripe = if ripe { wallet.outside_words } else { 2_500_000_000 };
    stage
}

/// What [`busy`] brings, less a new address or a watched one: in a sheet those come from the
/// person's own press on the button below the list, not on their own.
fn on_its_own(quiet: &Snapshot) -> Snapshot {
    let mut stage = busy(quiet);
    stage.node.keys = quiet.node.keys.clone();
    stage.node.watched = quiet.node.watched.clone();
    stage
}

/// The sheets the settings page opens, each with the wallet maturing under it both ways. A press on
/// the button that moves older coins to the words was lost on a slow runner as the last of them
/// matured under the sheet.
#[test]
fn what_the_core_says_leaves_every_control_in_a_sheet_where_it_is() {
    let mut window = harness(app(scratch("steady-sheets"), quiet_settings(), None), SIZES[0]);
    wait_until(&mut window, |app| !app.snapshot().node.address.is_empty());
    let base = (*window.state().snapshot()).clone();
    let waiting = wallet_moves(&base, false);
    let ripe = wallet_moves(&base, true);
    let open = word(&window, "common.open");
    let mut wrong = Vec::new();
    window.state_mut().open("page.settings");
    window.state_mut().set_stage(Some(waiting.clone()));
    window.run_steps(SETTLE_STEPS);
    let rows = window.query_all_by_label(&open).count();
    assert!(rows > 0, "the settings page opens no sheet");
    for row in 0..rows {
        window.state_mut().open("page.settings");
        window.state_mut().set_stage(Some(waiting.clone()));
        window.run_steps(SETTLE_STEPS);
        if let Some(button) = window.query_all_by_label(&open).nth(row) {
            button.click_accesskit();
        }
        window.run_steps(SETTLE_STEPS);
        compare(&mut window, &waiting, &on_its_own(&ripe), &format!("sheet {row}, coins matured"), &mut wrong);
        compare(&mut window, &ripe, &on_its_own(&waiting), &format!("sheet {row}, a coin mined"), &mut wrong);
        window.key_press(egui::Key::Escape);
        window.run_steps(SETTLE_STEPS);
    }
    assert!(wrong.is_empty(), "these did not hold still: {wrong:#?}");
}
