//! The rail's places as a person meets them: the same places in the same order in both modes,
//! OfflineCoin once on the first run, each task a few clicks from Home, and every explanation
//! short enough to stand in place.
use super::{app, harness, quiet_settings, scratch, staged, wait_until, Window, SETTLE_STEPS, SIZES};
use crate::discovery::{ChainGroup, CoinGroup, Entry, Identity, Relation, Work};
use crate::discovery::Reach;
use crate::i18n::{self, Lang, Mode, Voice};
use crate::pages::speed::cap_for;
use crate::settings::Settings;
use crate::ui::explain::fits_in_place;
use egui::accesskit::Role;
use egui_kittest::kittest::{By, Queryable};
use olc_core::CoinConfig;
use olc_node::Keeps;
use std::fs;
use std::path::Path;

/// The rail from top to bottom, in both modes.
const PLACES: [&str; 10] = [
    "page.home",
    "page.mining",
    "page.wallet",
    "page.records",
    "page.payments",
    "page.lab",
    "page.coins",
    "page.boards",
    "page.console",
    "page.settings",
];

fn word(window: &Window, key: &str) -> String {
    let settings = window.state().settings();
    i18n::text(Voice { lang: settings.language, mode: settings.mode }, key).to_string()
}

/// The rail's button for a place: the leftmost button with its name.
fn rail_button(window: &Window, key: &str) -> egui::Rect {
    let name = word(window, key);
    let buttons = window.query_all(By::new().role(Role::Button).label(name.as_str()));
    let rects = buttons.map(|node| node.rect());
    rects.min_by(|a, b| a.left().total_cmp(&b.left())).unwrap_or_else(|| panic!("{key} is not on the rail"))
}

#[test]
fn both_modes_have_the_same_places_in_the_same_order() {
    let mut window = harness(app(scratch("places"), quiet_settings(), None), SIZES[0]);
    assert_eq!(window.state().page_titles(), PLACES);
    for mode in [Mode::Basic, Mode::Expert] {
        for language in [Lang::En, Lang::Ko] {
            window.state_mut().settings_mut().mode = mode;
            window.state_mut().settings_mut().language = language;
            window.run_steps(SETTLE_STEPS);
            let tops: Vec<f32> = PLACES.iter().map(|key| rail_button(&window, key).top()).collect();
            assert!(tops.windows(2).all(|pair| pair[0] < pair[1]), "{mode:?} {language:?}: {tops:?}");
        }
    }
}

fn found_olc() -> CoinGroup {
    let olc = CoinConfig::olc();
    let entry = Entry {
        reach: Reach::ThisComputer,
        address: Some("127.0.0.1:1".into()),
        via: None,
        device: "computer".into(),
        name: "Desk".into(),
        identity: Some(Identity::of(olc.clone())),
        tip: None,
        height: Some(12),
        work_done: None,
        build: String::new(),
        work: Work::Idle,
        keeps: Keeps::Unsaid,
        age_secs: None,
    };
    let chain = ChainGroup { rules: None, coin: Some(olc.clone()), relation: Relation::Network, entries: vec![entry], tips: 1 };
    CoinGroup { identity: Some(Identity::of(olc)), chains: vec![chain] }
}

/// Another OLC Core on this computer is found, and OfflineCoin is still one choice that says so.
#[test]
fn offline_coin_shows_once_with_what_was_found_nearby() {
    let settings = Settings { setup_done: false, ..quiet_settings() };
    let mut window = harness(app(scratch("olc-once"), settings, None), SIZES[0]);
    let mut stage = staged(Vec::new());
    stage.groups = vec![found_olc()];
    window.state_mut().set_stage(Some(stage));
    window.run_steps(SETTLE_STEPS);
    let name = CoinConfig::olc().name;
    assert_eq!(window.query_all(By::new().role(Role::Button).label(name.as_str())).count(), 1);
    assert!(window.query_all_by_label_contains("Found nearby: 1").next().is_some());
}

/// Clicks from Home to the control that does each task of the audit, and whether the control
/// needs a scroll at 1280 by 800. Before: speed 2 and a scroll, the attack none in basic and 2
/// and a pick in expert, a miner 3, listening 2 and a scroll, the backup 2.
#[test]
fn every_task_is_a_few_clicks_from_home() {
    let mut window = harness(app(scratch("clicks"), quiet_settings(), None), SIZES[0]);
    wait_until(&mut window, |app| !app.snapshot().node.address.is_empty());
    let height = SIZES[0].1 as f32;
    let full = window.state().snapshot().full_rate;
    for mode in [Mode::Basic, Mode::Expert] {
        window.state_mut().settings_mut().mode = mode;
        let mut table = Vec::new();
        let mut reach = |window: &mut Window, task: &str, place: Option<&str>, control: &str| {
            window.state_mut().open("page.home");
            window.run_steps(SETTLE_STEPS);
            let mut clicks = 0;
            if let Some(place) = place {
                let rect = rail_button(window, place);
                window.query_all(By::new().role(Role::Button).label(word(window, place).as_str()))
                    .find(|node| node.rect() == rect)
                    .expect("the rail button")
                    .click_accesskit();
                window.run_steps(SETTLE_STEPS);
                clicks += 1;
            }
            let name = word(window, control);
            let found = window.query_all_by_label(&name).next().unwrap_or_else(|| panic!("{task}: no {control}"));
            let scroll = found.rect().bottom() > height;
            table.push(format!("{task}: {} click{}", clicks + 1, if scroll { " and a scroll" } else { "" }));
            (clicks, scroll)
        };
        let speed = reach(&mut window, "speed", None, "rate.full");
        let attack = reach(&mut window, "51% attack", Some("page.lab"), "lab2.start_attack");
        let miner = reach(&mut window, "add a miner", Some("page.mining"), "mining.add");
        let backup = reach(&mut window, "back up", Some("page.wallet"), "settings.backup_go");
        let listen = reach(&mut window, "listening", Some("page.settings"), "settings.listen_lan");
        eprintln!("{mode:?}: {}", table.join(", "));
        assert_eq!(speed, (0, false), "the speed is on Home");
        assert!(attack.0 <= 1 && !attack.1, "the attack starts from the Lab's first screen");
        assert!(miner.0 <= 1 && !miner.1, "a miner is added from the top of Mining");
        assert!(backup.0 <= 1 && !backup.1, "the backup is on the Wallet's first screen");
        assert!(listen.0 <= 1, "listening is in Settings");
    }
    // A step on Home sets the limit there, with no page between.
    window.state_mut().settings_mut().mode = Mode::Basic;
    window.state_mut().open("page.home");
    window.run_steps(SETTLE_STEPS);
    window.query_all_by_label("50%").next().expect("the half step").click_accesskit();
    window.run_steps(SETTLE_STEPS);
    assert_eq!(window.state_mut().settings_mut().hash_cap, cap_for(full, 50));
    wait_until(&mut window, |app| app.snapshot().node.cap == cap_for(full, 50));
}

fn sources(folder: &Path, found: &mut Vec<(String, String)>) {
    for entry in fs::read_dir(folder).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            sources(&path, found);
        } else if path.extension().is_some_and(|word| word == "rs") {
            found.push((path.display().to_string(), fs::read_to_string(&path).unwrap()));
        }
    }
}

/// The keys inside each `explain::short(` call, up to the end of its statement.
fn keys_in_short_calls(text: &str) -> Vec<String> {
    let mut keys = Vec::new();
    for (start, _) in text.match_indices("explain::short(") {
        let call = &text[start..];
        let call = &call[..call.find(";\n").unwrap_or(call.len())];
        keys.extend(call.split('"').skip(1).step_by(2).filter(|piece| is_key(piece)).map(str::to_string));
    }
    keys
}

fn is_key(piece: &str) -> bool {
    piece.split_once('.').is_some_and(|(head, tail)| {
        !head.is_empty() && !tail.is_empty() && piece.bytes().all(|byte| byte.is_ascii_lowercase() || byte == b'_' || byte == b'.')
    })
}

/// Explanations whose key is chosen before the call, so the call holds only a name.
const THROUGH_A_NAME: [&str; 5] = ["mining.pacing_about", "mining.pacing_each", "mining.pacing_fixed", "lab2.about_attack", "lab2.about_split"];

/// Every explanation that stands in place fits three short lines in each language and mode, with
/// its blanks filled by long values. The rest goes behind Details.
#[test]
fn every_explanation_in_place_fits_three_lines() {
    let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut texts = Vec::new();
    sources(&folder.join("pages"), &mut texts);
    sources(&folder.join("ui"), &mut texts);
    let mut keys: Vec<String> = texts.iter().flat_map(|(_, text)| keys_in_short_calls(text)).collect();
    // The Wallet's rows pass their keys through a name, and so do these.
    for (path, text) in &texts {
        if path.ends_with("wallet.rs") {
            keys.extend(text.split('"').skip(1).step_by(2).filter(|piece| is_key(piece) && piece.ends_with("_about")).map(str::to_string));
        }
    }
    keys.extend(THROUGH_A_NAME.iter().map(|key| (*key).to_string()));
    keys.sort();
    keys.dedup();
    assert!(keys.len() >= 15, "{keys:?}");
    let mut long = Vec::new();
    for lang in [Lang::En, Lang::Ko] {
        for mode in [Mode::Basic, Mode::Expert] {
            for key in &keys {
                let said = i18n::text(Voice { lang, mode }, key).replace("{}", "1,234,567");
                if said != "?" && !fits_in_place(&said) {
                    long.push(format!("{lang:?} {mode:?} {key}: {said}"));
                }
            }
        }
    }
    assert!(long.is_empty(), "too long to stand in place:\n{}", long.join("\n"));
}
