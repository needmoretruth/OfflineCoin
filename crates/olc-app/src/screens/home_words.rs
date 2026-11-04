//! The words line on Home: it waits for the wallet to load, and on the smallest window its text
//! wraps so the button beside it stays in sight. `OLC_WALLET_SHOTS` keeps the narrow lines as pictures.
use super::shots::shot;
use super::{app, harness, quiet_settings, scratch, wait_until, Window, SETTLE_STEPS, SIZES};
use crate::core::{Snapshot, Words};
use crate::i18n::{self, Lang, Voice};
use egui_kittest::kittest::Queryable;

/// The smallest window the app allows (app.rs `WINDOW_MIN`).
const NARROW: (u32, u32) = (960, 620);

fn word(window: &Window, key: &str) -> &'static str {
    let settings = window.state().settings();
    i18n::text(Voice { lang: settings.language, mode: settings.mode }, key)
}

/// The longest fixed part of a line, for finding it whatever amount fills it.
fn fixed_part(text: &str) -> &str {
    text.split("{}").max_by_key(|part| part.len()).unwrap_or(text).trim()
}

#[test]
fn the_no_words_line_waits_for_the_wallet() {
    let mut window = harness(app(scratch("words-wait"), quiet_settings(), None), SIZES[0]);
    window.state_mut().set_stage(Some(Snapshot::default()));
    window.state_mut().open("page.home");
    window.run_steps(SETTLE_STEPS);
    let none = fixed_part(word(&window, "home.words_none"));
    assert!(window.query_all_by_label_contains(none).next().is_none(), "Home says no words before the wallet has loaded");
    assert!(window.query_by_label(word(&window, "words.make")).is_none(), "the words button stands before the wallet has loaded");
}

#[test]
fn every_words_line_wraps_beside_its_button_on_the_smallest_window() {
    let pictures = std::env::var_os("OLC_WALLET_SHOTS").map(std::path::PathBuf::from);
    let mut window = harness(app(scratch("words-narrow"), quiet_settings(), None), NARROW);
    wait_until(&mut window, |app| !app.snapshot().node.address.is_empty());
    let loaded = (*window.state().snapshot()).clone();
    let twelve = Words((1..=12).map(|n| format!("word{n}")).collect());
    let mut wrong = Vec::new();
    for lang in [Lang::En, Lang::Ko] {
        window.state_mut().settings_mut().language = lang;
        let cases = [("unwritten", "home.words_unwritten", "home.words_write"), ("none", "home.words_none", "words.make"), ("outside", "home.words_outside", "common.open")];
        for (name, line_key, button_key) in cases {
            let mut stage = loaded.clone();
            let wallet = &mut stage.node.wallet;
            wallet.words_to_write = (name == "unwritten").then(|| twelve.clone());
            wallet.has_words = name != "none";
            wallet.outside_words = if name == "outside" { 2_099_999_997_690_000 } else { 0 };
            window.state_mut().set_stage(Some(stage));
            window.state_mut().open("page.home");
            window.run_steps(SETTLE_STEPS);
            let place = format!("{lang:?} {name}");
            let Some(line) = window.query_all_by_label_contains(fixed_part(word(&window, line_key))).next().map(|node| node.rect()) else {
                wrong.push(format!("{place}: no line"));
                continue;
            };
            let Some(button) = window.query_all_by_label(word(&window, button_key)).next().map(|node| node.rect()) else {
                wrong.push(format!("{place}: no button"));
                continue;
            };
            if button.max.x > NARROW.0 as f32 || line.max.x > button.min.x {
                wrong.push(format!("{place}: line {line:?}, button {button:?}"));
            }
            if let (Some(folder), "none") = (&pictures, name) {
                shot(&mut window, folder, &format!("home-no-words-narrow-{}", format!("{lang:?}").to_lowercase()));
            }
        }
    }
    assert!(wrong.is_empty(), "these lines push their button off or under the text: {wrong:#?}");
}

/// The coin file OfflineCoin 0.2 wrote for its built-in OLC.
const OLC_0_2: &str = r#"{"name": "OfflineCoin", "ticker": "OLC", "message": "OLCC genesis", "genesis_time_ms": 1793491200000, "initial_difficulty": 900000, "premine": [], "rules": {"block_time_ms": 30000, "retarget_interval": 10, "reward": 5000000000, "halving_interval": 210000, "coinbase_maturity": 3, "max_block_bytes": 4096, "min_fee": 0, "min_difficulty": 30000, "floor_from_height": 20000}, "message_in_block": false, "upgrade_from": 6240}"#;

#[test]
fn home_says_why_an_app_from_0_2_starts_again_at_zero_until_closed() {
    let root = scratch("old-olc");
    let old = root.join("coins").join("82816fe40cdb");
    std::fs::create_dir_all(&old).unwrap();
    std::fs::write(old.join("coin.json"), OLC_0_2).unwrap();
    let settings = crate::settings::Settings { coin: Some("82816fe40cdb".into()), ..quiet_settings() };
    let mut window = harness(app(root.clone(), settings, None), SIZES[0]);
    window.state_mut().open("page.home");
    wait_until(&mut window, |app| app.snapshot().old_olc_note);
    let line = word(&window, "home.old_olc");
    assert!(window.query_by_label(line).is_some(), "Home does not say that OLC started over");
    let coin = window.state().snapshot().node.coin.clone().expect("a coin");
    assert!(!coin.is_retired_olc(), "the app stayed on the old OLC");

    let close = word(&window, "common.close");
    window.query_all_by_label(close).next().expect("the line's close button").click();
    wait_until(&mut window, |app| !app.snapshot().old_olc_note);
    assert!(window.query_by_label(line).is_none(), "the line stays after closing");
    assert!(!crate::settings::Settings::load(&root).old_olc_note, "the closed line comes back at the next start");
}
