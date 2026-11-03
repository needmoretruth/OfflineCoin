//! Mining together lives on Mining in both modes: under the miners and boards, a person finds how
//! to open a pool on this computer and how to join one on another, and Devices has no tab for it.
//! With `OLC_SHOTS` it keeps a picture of that place in each mode and of a pool that runs, with
//! this computer mining in it.
use super::shots::shot;
use super::{app, harness, quiet_settings, scratch, Window, SETTLE_STEPS};
use crate::core::{computer_name, worker_part, BlockState, ClientView, FoundRow, Join, Link, LinkRow, Snapshot, Who, WorkerRow};
use olc_core::coin::COIN;
use crate::i18n::{self, Lang, Mode, Voice};
use crate::settings::{Settings, Theme};
use egui_kittest::kittest::Queryable;
use std::path::Path;
use std::time::Duration;

/// How long the core may take to list the miners: 200 steps of 50 ms.
const WAITS: u32 = 200;
const WAIT_STEP: Duration = Duration::from_millis(50);

fn word(window: &Window, key: &str) -> &'static str {
    let settings = window.state().settings();
    i18n::text(Voice { lang: settings.language, mode: settings.mode }, key)
}

fn shows(window: &Window, key: &str) -> bool {
    window.query_all_by_label(word(window, key)).next().is_some()
}

fn picture(window: &mut Window, name: &str) {
    if let Ok(folder) = std::env::var("OLC_SHOTS") {
        shot(window, Path::new(&folder), name);
    }
}

/// Mining, scrolled to where mining together starts, as a person scrolls down to it.
fn on_mining(lang: Lang, mode: Mode) -> Window {
    let settings = Settings { language: lang, mode, theme: Theme::Dark, ..quiet_settings() };
    let mut window = harness(app(scratch("pool-place"), settings, None), (1280, 800));
    window.state_mut().open("page.mining");
    // The core lists the miners that could work a moment after it starts, this computer first.
    for _ in 0..WAITS {
        if !window.state().snapshot().pool.choices.is_empty() {
            break;
        }
        std::thread::sleep(WAIT_STEP);
        window.run_steps(1);
    }
    window.run_steps(SETTLE_STEPS);
    let title = word(&window, "mining.together");
    window.query_all_by_label(title).next().unwrap_or_else(|| panic!("{title} is not on Mining")).scroll_to_me();
    window.run_steps(SETTLE_STEPS);
    window
}

#[test]
fn mining_together_is_on_mining_in_both_modes() {
    for (lang, mode, name) in [(Lang::Ko, Mode::Basic, "basic-ko-dark"), (Lang::En, Mode::Expert, "expert-en-dark")] {
        let mut window = on_mining(lang, mode);
        for key in ["pool.run_here", "pool.start", "pool.join_title", "pool.this_computer"] {
            assert!(shows(&window, key), "{name}: Mining has {key}");
        }
        picture(&mut window, &format!("mining-pool-{name}"));
        // Further down, joining a pool on another computer.
        if let Some(node) = window.query_all_by_label(word(&window, "pool.join_title")).next() {
            node.scroll_to_me();
        }
        window.run_steps(SETTLE_STEPS);
        picture(&mut window, &format!("mining-pool-join-{name}"));
        window.state_mut().open("page.boards");
        window.run_steps(SETTLE_STEPS);
        for key in ["mining.together", "pool.run_here", "pool.join_title"] {
            assert!(!shows(&window, key), "{name}: Devices no longer holds {key}");
        }
    }

    // A pool that runs is the whole section: who works in it and what they were paid. This
    // computer's miner works in it, so the computer's card counts its speed and says so.
    let mut window = on_mining(Lang::Ko, Mode::Basic);
    let stage = running_pool(&window.state().snapshot());
    window.state_mut().set_stage(Some(stage));
    window.run_steps(SETTLE_STEPS);
    let line = format!("{}: {}", word(&window, "mining.this_app"), word(&window, "pool.in_pool_here"));
    assert!(window.query_all_by_label(&line).next().is_some(), "the miners table says this computer mines in the pool");
    let title = word(&window, "mining.together");
    if let Some(node) = window.query_all_by_label(title).next() {
        node.scroll_to_me();
    }
    window.run_steps(SETTLE_STEPS);
    assert!(shows(&window, "pool.running") && shows(&window, "pool.stop"), "a running pool shows itself and its stop");
    picture(&mut window, "mining-pool-running-basic-ko-dark");
    // The pool sees shares, not hashes: an expert's speed column says it is an estimate, and how.
    let settings = window.state_mut().settings_mut();
    (settings.mode, settings.language) = (Mode::Expert, Lang::En);
    window.run_steps(SETTLE_STEPS);
    assert!(window.query_all_by_label("Est. hashrate").next().is_some(), "an expert reads the speed as estimated");
    assert!(shows(&window, "pool.speed_note"), "and from what it is estimated");
    if let Some(node) = window.query_all_by_label("Est. hashrate").next() {
        node.scroll_to_me();
    }
    window.run_steps(SETTLE_STEPS);
    picture(&mut window, "mining-pool-running-expert-en-dark");
    window.state_mut().open("page.home");
    window.run_steps(SETTLE_STEPS);
    assert!(shows(&window, "home.mining_pool"), "Home says this computer mines in a pool");
}

/// A pool this computer runs and mines in, with another computer, after two blocks shared out:
/// what each was paid adds up to what the blocks brought, and this computer's speed is the one
/// its card counts.
fn running_pool(live: &Snapshot) -> Snapshot {
    // Under the 20,000 a quiet window keeps to, as a real miner stays under its limit.
    const SPEED: u64 = 19_000;
    const OTHER_SPEED: u64 = 36_000;
    const REWARD: u64 = 50 * COIN;
    let mut stage = live.clone();
    stage.node.mining = true;
    stage.node.hashrate = SPEED;
    stage.node.chain.hashrate_here = SPEED;
    let now = crate::core::unix_ms();
    stage.node.chain.trail = (0..=36).map(|step| (now - (36 - step) * 5_000, SPEED)).collect();
    let me = format!("{}.{}", stage.node.miner, worker_part(&computer_name()));
    let (my_part, other_part) = (REWARD / 100 * 53, REWARD / 100 * 47);
    let row = |name: String, worker: String, speed, accepted, paid, here| WorkerRow {
        name,
        worker,
        speed,
        last_share_secs: Some(2),
        accepted,
        paid,
        online: true,
        here,
        ..WorkerRow::default()
    };
    stage.pool.running = true;
    stage.pool.port = 3333;
    stage.pool.reach = vec!["127.0.0.1:3333".into()];
    stage.pool.workers = vec![
        row(me.clone(), worker_part(&computer_name()), SPEED, 1_210, 2 * my_part, Some(Who::Own)),
        row("olc1example.Kitchen".into(), "Kitchen".into(), OTHER_SPEED, 1_080, 2 * other_part, None),
    ];
    let block = |height, secs_ago| FoundRow {
        height,
        value: REWARD,
        state: BlockState::Paid,
        pool_fee: 0,
        secs_ago,
        parts: vec![(me.clone(), my_part), ("olc1example.Kitchen".into(), other_part)],
    };
    stage.pool.found = vec![block(41, 95), block(28, 610)];
    let join = Join { who: Who::Own, address: "127.0.0.1:3333".into(), user: me.clone() };
    let view = ClientView { link: Link::Working, accepted: 1_210, ..ClientView::default() };
    stage.pool.links = vec![LinkRow { join, label: String::new(), view, trouble: None, here: true }];
    stage
}
