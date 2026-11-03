//! Board zero is heard only through board one, the board on the cable. Before both boards are
//! updated its row says why it cannot work; after, it is a worker of the pool here, and what each
//! worker was paid reads to two decimals in basic mode and in full for an expert. Staged, since no
//! test may touch a board: `crates/olc-service/tests/pool_relay.rs` runs the same rows for real.
//! A board that never took the work says why under its row, and its switch goes off.
//! The pictures go to `.scratch/live/pool-shots`, at 1280 by 800, dark.
use super::live::live_folder;
use super::shots::{shot, tap};
use super::{app, harness, quiet_settings, scratch, wait_until, Window, SETTLE_STEPS};
use crate::core::{BlockState, Choice, FoundRow, PoolSettings, Snapshot, SwitchedOff, Who, WorkerRow};
use crate::i18n::{self, Lang, Mode, Voice};
use egui::accesskit::{Role, Toggled};
use egui_kittest::kittest::{By, NodeT, Queryable};

const SIZE: (u32, u32) = (1280, 800);
const ONE: [u8; 6] = [0xAC, 0x27, 0x6E, 0xAA, 0xE2, 0x04];
const TWO: [u8; 6] = [0xAC, 0x27, 0x6E, 0xAA, 0xEF, 0x78];
const ZERO: [u8; 20] = [0x5A; 20];
/// The shares each worker sent since the pool started, all for the one block it found, which pays
/// each in proportion.
const SHARES: [(&str, u64); 4] = [("core", 412), ("보드원", 461), ("보드투", 367), ("보드제로", 360)];
const BLOCK: u64 = 5_000_000_000;

fn choice(who: Option<Who>, label: &str, through: Option<&str>, why_not: Option<&'static str>) -> Choice {
    Choice { who, label: label.into(), through: through.map(Into::into), why_not, speed: 9_000 }
}

/// The pool page before Start, with board zero's row as the two builds make it.
fn choosing(live: &Snapshot, updated: bool) -> Snapshot {
    let zero = match updated {
        true => choice(Some(Who::Lent(ZERO)), "보드제로", Some("보드원"), None),
        false => choice(None, "보드제로", Some("보드원"), Some("pool.why_heard_only")),
    };
    let mut stage = live.clone();
    stage.pool.running = false;
    stage.pool.choices = vec![
        Choice { speed: 40_000, ..choice(Some(Who::Own), "", None, None) },
        choice(Some(Who::Board(ONE)), "보드원", None, None),
        choice(Some(Who::Board(TWO)), "보드투", None, None),
        zero,
    ];
    stage
}

/// The pool running with a fee of `fee_bps`, one block found and shared out in proportion.
fn paid(live: &Snapshot, fee_bps: u32) -> Snapshot {
    let total: u64 = SHARES.iter().map(|(_, shares)| shares).sum();
    let fee = BLOCK * u64::from(fee_bps) / 10_000;
    let per_share = (BLOCK - fee) / total;
    assert_eq!(per_share * total, BLOCK - fee, "the stage shares out to the last unit");
    let here = [Who::Own, Who::Board(ONE), Who::Board(TWO), Who::Lent(ZERO)];
    let workers = SHARES.iter().zip(here).map(|((worker, shares), who)| WorkerRow {
        name: format!("olc1example.{worker}"),
        worker: (*worker).into(),
        address: "olc1example".into(),
        speed: 9_000,
        last_share_secs: Some(2),
        accepted: *shares,
        paid: shares * per_share,
        difficulty: 500,
        online: true,
        here: Some(who),
        ..WorkerRow::default()
    });
    let mut stage = live.clone();
    stage.pool.running = true;
    stage.pool.port = 3333;
    stage.pool.fee_bps = fee_bps;
    stage.pool.reach = vec!["127.0.0.1:3333".into()];
    stage.pool.workers = workers.collect();
    let parts = SHARES.iter().map(|(worker, shares)| (format!("olc1example.{worker}"), shares * per_share)).collect();
    stage.pool.found = vec![FoundRow { height: 1_204, value: BLOCK, state: BlockState::Paid, pool_fee: fee, secs_ago: 40, parts }];
    stage
}

fn speak(window: &mut Window, mode: Mode, lang: Lang) {
    let settings = window.state_mut().settings_mut();
    settings.mode = mode;
    settings.language = lang;
    window.run_steps(SETTLE_STEPS);
}

fn texts(window: &Window) -> Vec<String> {
    let all = window.query_all(By::new().predicate(|_| true));
    all.filter_map(|node| node.accesskit_node().label().or_else(|| node.accesskit_node().value())).collect()
}

fn says(window: &Window, part: &str) -> bool {
    texts(window).iter().any(|text| text.contains(part))
}

fn save(window: &mut Window, name: &str) {
    shot(window, &live_folder().join("pool-shots"), name);
}

#[test]
fn board_zero_works_through_board_one_and_payouts_read_to_two_decimals() {
    let mut settings = quiet_settings();
    settings.pool = PoolSettings { workers: vec![Who::Own, Who::Board(ONE), Who::Board(TWO), Who::Lent(ZERO)], ..PoolSettings::default() };
    let mut window = harness(app(scratch("pool-relay-shots"), settings, None), SIZE);
    wait_until(&mut window, |app| !app.snapshot().node.address.is_empty());
    let live = window.state().snapshot();
    speak(&mut window, Mode::Basic, Lang::Ko);
    window.state_mut().set_stage(Some(choosing(&live, false)));
    window.state_mut().open("page.boards");
    window.run_steps(SETTLE_STEPS);
    tap(&mut window, "boards.tab_pool");
    save(&mut window, "relay-before-update-basic-ko");
    assert!(says(&window, "보드원 통해 연결"), "{:#?}", texts(&window));
    assert!(says(&window, "둘 다 업데이트해야"), "{:#?}", texts(&window));

    window.state_mut().set_stage(Some(choosing(&live, true)));
    window.run_steps(SETTLE_STEPS);
    save(&mut window, "relay-choosing-basic-ko");
    assert!(!says(&window, "둘 다 업데이트해야"), "board zero still has a reason");

    // The default fee is 0 %, so the block goes to the workers whole and no pool share shows.
    let ticker = live.node.coin.as_ref().map_or(String::new(), |coin| coin.ticker.clone());
    window.state_mut().set_stage(Some(paid(&live, 0)));
    window.run_steps(SETTLE_STEPS);
    save(&mut window, "relay-paid-basic-ko");
    assert!(says(&window, &format!("14.41 {ticker}")), "{:#?}", texts(&window));
    assert!(!says(&window, "14.40625"), "basic mode shows every decimal");
    assert!(!says(&window, i18n::text(Voice { lang: Lang::Ko, mode: Mode::Basic }, "pool.kept")), "a pool share of nothing");

    // An expert set a fee of 1 %: the pool's share is a thin grey line and every unit shows.
    speak(&mut window, Mode::Expert, Lang::En);
    window.state_mut().set_stage(Some(paid(&live, 100)));
    window.run_steps(SETTLE_STEPS);
    save(&mut window, "relay-paid-expert-en");
    assert!(says(&window, &format!("14.2621875 {ticker}")), "{:#?}", texts(&window));
    assert!(says(&window, i18n::text(Voice { lang: Lang::En, mode: Mode::Expert }, "pool.kept")), "{:#?}", texts(&window));
}

/// The pool switched board zero off: it never said it took the work.
fn switched_off(mut stage: Snapshot, at_ms: u64) -> Snapshot {
    let user = "olc1example.보드제로".to_string();
    stage.pool.switched_off = vec![SwitchedOff { who: Who::Lent(ZERO), user: user.clone(), why: "pool.lent_untrusted", relay: "보드원".into(), at_ms }];
    for row in stage.pool.workers.iter_mut().filter(|row| row.name == user) {
        (row.online, row.speed, row.accepted, row.paid, row.last_share_secs) = (false, 0, 0, 0, None);
    }
    stage
}

fn switch_of(window: &Window, label: &str) -> Option<bool> {
    let mut found = window.query_all(By::new().role(Role::Switch).label(label));
    found.next().map(|node| matches!(node.accesskit_node().toggled(), Some(Toggled::True)))
}

#[test]
fn a_board_that_never_took_the_work_says_why_and_its_switch_goes_off() {
    let mut settings = quiet_settings();
    settings.pool = PoolSettings { workers: vec![Who::Own, Who::Lent(ZERO)], ..PoolSettings::default() };
    let mut window = harness(app(scratch("pool-relay-off-shots"), settings, None), SIZE);
    wait_until(&mut window, |app| !app.snapshot().node.address.is_empty());
    let live = window.state().snapshot();
    speak(&mut window, Mode::Basic, Lang::Ko);
    window.state_mut().set_stage(Some(choosing(&live, true)));
    window.state_mut().open("page.boards");
    window.run_steps(SETTLE_STEPS);
    tap(&mut window, "boards.tab_pool");
    assert_eq!(switch_of(&window, "보드제로"), Some(true), "board zero was chosen");
    let said = i18n::text(Voice { lang: Lang::Ko, mode: Mode::Basic }, "pool.lent_untrusted");

    // Running for a quarter minute, nothing found yet: board zero's row is grey, and the line
    // under it says why and what to do.
    let mut started = paid(&live, 0);
    started.pool.found.clear();
    for row in &mut started.pool.workers {
        (row.paid, row.accepted) = (0, row.accepted / 40);
    }
    window.state_mut().set_stage(Some(switched_off(started, 1_000)));
    window.run_steps(SETTLE_STEPS);
    save(&mut window, "relay-untrusted-running-basic-ko");
    assert!(says(&window, said), "{:#?}", texts(&window));

    // Stopped: its switch is off and says why; switched on again, it stays on.
    window.state_mut().set_stage(Some(switched_off(choosing(&live, true), 1_000)));
    window.run_steps(SETTLE_STEPS);
    save(&mut window, "relay-untrusted-stopped-basic-ko");
    assert_eq!(switch_of(&window, "보드제로"), Some(false), "the pool switched it off");
    assert!(says(&window, said), "{:#?}", texts(&window));
    tap_switch(&mut window, "보드제로");
    assert_eq!(switch_of(&window, "보드제로"), Some(true), "switched on again by hand");
    assert!(!says(&window, said), "the reason went on showing once it was switched on to try again");

    speak(&mut window, Mode::Expert, Lang::En);
    window.state_mut().set_stage(Some(switched_off(choosing(&live, true), 2_000)));
    window.run_steps(SETTLE_STEPS);
    save(&mut window, "relay-untrusted-stopped-expert-en");
    assert_eq!(switch_of(&window, "보드제로"), Some(false), "a second switch-off turns it off again");
}

fn tap_switch(window: &mut Window, label: &str) {
    let Some(switch) = window.query_all(By::new().role(Role::Switch).label(label)).next() else {
        panic!("{label} has no switch");
    };
    switch.click_accesskit();
    window.run_steps(SETTLE_STEPS);
}
