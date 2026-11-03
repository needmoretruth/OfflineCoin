//! Every place of the rail on a real race: this app and two more miners on this computer find
//! blocks on a fast coin, and a board stands staged beside them. Home shows who found each block
//! and the speed control; Mining shows the race. With `OLC_SHOTS` set every page is saved at
//! 1280 by 800, dark in basic Korean and expert English, and Home and Mining light as well.
use super::{app, harness, node_program, port_of_scene, quiet_settings, scratch, staged_board, Window, SETTLE_STEPS, SIZES};
use crate::core::{Command, Snapshot};
use crate::i18n::{Lang, Mode};
use crate::local::{NodeSpec, Preset};
use crate::settings::{Settings, Theme};
use egui_kittest::kittest::Queryable;
use olc_core::CoinConfig;
use std::path::Path;
use std::time::{Duration, Instant};

const BLOCK_MS: u64 = 2_000;
const APP_CAP: u64 = 3_000;
const A_CAP: u64 = 4_000;
const B_CAP: u64 = 2_000;

fn miner(id: u32, name: &str, cap: u64) -> NodeSpec {
    NodeSpec { name: name.into(), mining: true, hash_cap: cap, joined: true, ..Preset::Custom.spec(id) }
}

#[track_caller]
fn wait_long(window: &mut Window, what: &str, seconds: u64, done: impl Fn(&Snapshot) -> bool) {
    let start = Instant::now();
    loop {
        window.run_steps(1);
        if done(&window.state().snapshot()) {
            return;
        }
        assert!(start.elapsed() < Duration::from_secs(seconds), "waited {seconds} s for {what}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn look(window: &mut Window, mode: Mode, language: Lang, theme: Theme) {
    let settings = window.state_mut().settings_mut();
    settings.mode = mode;
    settings.language = language;
    settings.theme = theme;
    window.run_steps(SETTLE_STEPS);
}

fn save(window: &mut Window, folder: Option<&Path>, name: &str) {
    if let Some(folder) = folder {
        super::shots::shot(window, folder, name);
    }
}

#[test]
fn every_place_on_a_real_race() {
    let folder = std::env::var_os("OLC_SHOTS").map(std::path::PathBuf::from);
    let settings = Settings { port: port_of_scene(64), mine_on_start: true, ..quiet_settings() };
    let mut window = harness(app(scratch("place-shots"), settings, Some(node_program())), SIZES[0]);
    let mut coin = CoinConfig::olc();
    coin.name = "Race".into();
    coin.ticker = "RCE".into();
    coin.rules.block_time_ms = BLOCK_MS;
    coin.rules.min_difficulty = 100;
    coin.rules.retarget_interval = 20;
    coin.initial_difficulty = (APP_CAP + A_CAP + B_CAP) * BLOCK_MS / 1_000;
    window.state().send(Command::Join { coin: Box::new(coin), address: None });
    window.state().send(Command::SetCap(APP_CAP));
    window.state().send(Command::SetMining(true));
    window.state().send(Command::AddNode(Box::new(miner(61, "Miner A", A_CAP))));
    window.state().send(Command::AddNode(Box::new(miner(62, "Miner B", B_CAP))));
    wait_long(&mut window, "a race with blocks of this app", 240, |snap| {
        let node = &snap.node;
        node.chain.latest.len() >= 10 && node.chain.shares.len() >= 2 && node.blocks_mined >= 2 && !node.activity.is_empty()
    });
    window.run_steps(SETTLE_STEPS);

    // The live view with one board staged beside it, held still for the pictures.
    let mut stage = Snapshot::clone(&window.state().snapshot());
    stage.boards = vec![staged_board(true, false)];
    window.state_mut().set_stage(Some(stage));
    look(&mut window, Mode::Basic, Lang::En, Theme::Dark);
    window.state_mut().open("page.home");
    window.run_steps(SETTLE_STEPS);
    let shows = |window: &Window, text: &str| window.query_all_by_label_contains(text).next().is_some();
    assert!(shows(&window, "Miner A") || shows(&window, "Miner B"), "Home names who found the blocks");
    assert!(shows(&window, "50%"), "the speed steps are on Home");
    window.state_mut().open("page.mining");
    window.run_steps(SETTLE_STEPS);
    assert!(shows(&window, "Who finds blocks"));
    assert!(shows(&window, "Miner A") && shows(&window, "Miner B"), "the race names every miner");

    let titles = window.state().page_titles();
    for (mode, language, theme, tag) in [
        (Mode::Basic, Lang::Ko, Theme::Dark, "basic-ko-dark"),
        (Mode::Expert, Lang::En, Theme::Dark, "expert-en-dark"),
        (Mode::Basic, Lang::Ko, Theme::Light, "basic-ko-light"),
        (Mode::Expert, Lang::En, Theme::Light, "expert-en-light"),
    ] {
        look(&mut window, mode, language, theme);
        for title in &titles {
            if theme == Theme::Light && !matches!(*title, "page.home" | "page.mining") {
                continue;
            }
            window.state_mut().open(title);
            window.run_steps(SETTLE_STEPS);
            let place = title.trim_start_matches("page.");
            save(&mut window, folder.as_deref(), &format!("{place}-{tag}"));
        }
    }
    // The whole of Home and Mining, for reading below the fold.
    look(&mut window, Mode::Basic, Lang::Ko, Theme::Dark);
    window.set_size(egui::vec2(1280.0, 1500.0));
    for title in ["page.home", "page.mining", "page.wallet"] {
        window.state_mut().open(title);
        window.run_steps(SETTLE_STEPS);
        let place = title.trim_start_matches("page.");
        save(&mut window, folder.as_deref(), &format!("{place}-basic-ko-dark-whole"));
    }
    window.state_mut().set_stage(None);
    window.state().send(Command::RemoveNode(61));
    window.state().send(Command::RemoveNode(62));
}
