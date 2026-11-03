//! Mining together lives on Mining in both modes: under the miners and boards, a person finds how
//! to open a pool on this computer and how to join one on another, and Devices has no tab for it.
//! With `OLC_SHOTS` it keeps a picture of that place in each mode and of a pool that runs.
use super::shots::shot;
use super::{app, harness, quiet_settings, scratch, Window, SETTLE_STEPS};
use crate::core::WorkerRow;
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

    // A pool that runs is the whole section: who works in it and what they were paid.
    let mut window = on_mining(Lang::Ko, Mode::Basic);
    let mut stage = (*window.state().snapshot()).clone();
    let worker = |name: &str, accepted, paid| WorkerRow {
        name: format!("olc1example.{name}"),
        worker: name.into(),
        speed: 40_000,
        last_share_secs: Some(3),
        accepted,
        paid,
        online: true,
        ..WorkerRow::default()
    };
    stage.pool.running = true;
    stage.pool.port = 3333;
    stage.pool.reach = vec!["127.0.0.1:3333".into()];
    stage.pool.workers = vec![worker("Kitchen", 1_204, 620_000_000), worker("Hallway", 880, 410_000_000)];
    window.state_mut().set_stage(Some(stage));
    window.run_steps(SETTLE_STEPS);
    let title = word(&window, "mining.together");
    if let Some(node) = window.query_all_by_label(title).next() {
        node.scroll_to_me();
    }
    window.run_steps(SETTLE_STEPS);
    assert!(shows(&window, "pool.running") && shows(&window, "pool.stop"), "a running pool shows itself and its stop");
    picture(&mut window, "mining-pool-running-basic-ko-dark");
}
