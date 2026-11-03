//! Settings as a person meets it: four groups, every setting saying when it takes hold, the two
//! comparisons behind Details, and a reset that asks a second time.
use super::shots::shot;
use super::{app, harness, port_of_scene, quiet_settings, scratch, Window, SETTLE_STEPS};
use crate::i18n::{Lang, Mode};
use crate::settings::Settings;
use egui_kittest::kittest::Queryable;
use std::path::Path;

fn picture(window: &mut Window, name: &str) {
    if let Ok(folder) = std::env::var("OLC_SHOTS") {
        shot(window, Path::new(&folder), name);
    }
}

fn press(window: &mut Window, label: &str, index: usize) {
    let found = window.query_all_by_label(label).nth(index);
    found.unwrap_or_else(|| panic!("{label} is not on the screen")).click_accesskit();
    window.run_steps(SETTLE_STEPS);
}

/// The tags under the names: how many say now and how many the next start.
fn tags(window: &Window, now: &str, later: &str) -> (usize, usize) {
    (window.query_all_by_label(now).count(), window.query_all_by_label(later).count())
}

#[test]
fn every_setting_says_when_it_takes_hold() {
    let settings = Settings { port: port_of_scene(86), language: Lang::En, ..quiet_settings() };
    let mut window = harness(app(scratch("settings-groups"), settings, None), (1280, 2400));
    window.state_mut().open("page.settings");
    window.run_steps(SETTLE_STEPS);
    for group in ["General", "Mining", "Network", "Display"] {
        assert!(window.query_all_by_label(group).count() > 0, "the {group} group");
    }
    // Basic: mode, keeping, merging and board updates; speed, mining at opening, where coins go
    // and the words in a block; listening, looking, links and Bluetooth when there is a switch for
    // it; and the four of Display.
    let bluetooth = usize::from(window.state().snapshot().node.bluetooth.is_some());
    assert_eq!(tags(&window, "Applies now", "From the next start"), (14 + bluetooth, 1));
    assert!(window.query_by_label("Change in Mining").is_some(), "the speed row goes to Mining");
    for gone in ["Back up", "Restore", "Seed words", "Half"] {
        assert!(window.query_by_label(gone).is_none(), "{gone} is not a preference");
    }

    // The three ways to keep blocks, on the same four criteria, opened under their own line.
    press(&mut window, "Details", 1);
    let keep = window.get_by_label("What to keep").rect();
    let table = window.get_by_label("Blocks for a new computer").rect();
    let merge = window.query_all_by_label("Merge small coins").map(|node| node.rect().top()).fold(f32::MAX, f32::min);
    assert!(keep.bottom() < table.top() && table.bottom() < merge, "the comparison sits under its line");
    for criterion in ["Mining", "Disk", "Blocks for a new computer", "Who checks the coins"] {
        assert!(window.query_all_by_label(criterion).count() > 0, "the comparison has {criterion}");
    }
    assert!(window.query_by_label("Recent blocks only").is_some());
    picture(&mut window, "settings-keeping-basic-en-dark");
    press(&mut window, "Hide details", 0);

    // Going back to defaults asks a second time and can be called off.
    press(&mut window, "Reset to defaults", 0);
    assert!(window.query_by_label("Reset everything").is_some(), "the second press resets");
    assert!(window.query_by_label_contains("Language, wallet and blocks stay").is_some());
    picture(&mut window, "settings-reset-basic-en-dark");
    press(&mut window, "Cancel", 0);
    assert!(window.query_by_label("Reset to defaults").is_some());

    // Expert adds threads, the broadcast, the port, which waits for the next start, the ports
    // to look on, the ESP-NOW card and signing the old way, and compares the two modes.
    window.state_mut().settings_mut().mode = Mode::Expert;
    window.run_steps(SETTLE_STEPS);
    assert_eq!(tags(&window, "Applies now", "From the next start"), (19 + bluetooth, 2));
    press(&mut window, "Details", 0);
    for criterion in ["Words", "What shows", "Where things are"] {
        assert!(window.query_by_label(criterion).is_some(), "the modes are compared on {criterion}");
    }
    picture(&mut window, "settings-modes-expert-en-dark");
}
