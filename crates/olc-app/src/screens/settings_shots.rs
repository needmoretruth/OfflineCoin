//! Settings as a person meets it: four groups, one line saying changes apply at once and a tag on
//! each that waits for the next start, the two comparisons behind Details, and a reset that asks
//! a second time.
use super::shots::{shot, with_boards};
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

#[test]
fn every_setting_says_when_it_takes_hold() {
    let settings = Settings { port: port_of_scene(86), language: Lang::En, ..quiet_settings() };
    let mut window = harness(app(scratch("settings-groups"), settings, None), (1280, 2400));
    window.state_mut().open("page.settings");
    window.run_steps(SETTLE_STEPS);
    for group in ["General", "Mining", "Network", "Display"] {
        assert!(window.query_all_by_label(group).count() > 0, "the {group} group");
    }
    // The page says once that changes apply at once; only mining at opening waits, and says so.
    assert!(window.query_by_label("Changes apply at once unless a row says otherwise.").is_some());
    assert_eq!(window.query_all_by_label("From the next start").count(), 1);
    assert!(window.query_by_label("Change in Mining").is_some(), "the speed row goes to Mining");
    for gone in ["Back up", "Restore", "Seed words", "Half", "Merge small coins", "Update boards on their own"] {
        assert!(window.query_by_label(gone).is_none(), "{gone} is not a preference");
    }

    // The three ways to keep blocks, on the same four criteria, opened under their own line.
    press(&mut window, "Details", 1);
    let keep = window.get_by_label("What to keep").rect();
    let table = window.get_by_label("Blocks for a new computer").rect();
    let files = window.get_by_label("Your files").rect();
    assert!(keep.bottom() < table.top() && table.bottom() < files.top(), "the comparison sits under its line");
    for criterion in ["Mining", "Disk", "Blocks for a new computer", "Who checks the coins"] {
        assert!(window.query_all_by_label(criterion).count() > 0, "the comparison has {criterion}");
    }
    assert!(window.query_by_label("Recent blocks only").is_some());
    picture(&mut window, "settings-keeping-basic-en-dark");
    press(&mut window, "Hide details", 0);

    // Going back to defaults asks a second time and can be called off.
    press(&mut window, "Reset to defaults", 0);
    assert!(window.query_by_label("Reset").is_some(), "the second press resets");
    assert!(window.query_by_label_contains("except the language").is_some());
    assert!(window.query_by_label_contains("Your wallet and blocks stay").is_some());
    picture(&mut window, "settings-reset-basic-en-dark");
    press(&mut window, "Cancel", 0);
    assert!(window.query_by_label("Reset to defaults").is_some());

    // Expert adds threads, the broadcast, the port, which waits for the next start, the ports
    // to look on and the ESP-NOW card, and compares the two modes.
    window.state_mut().settings_mut().mode = Mode::Expert;
    window.run_steps(SETTLE_STEPS);
    assert_eq!(window.query_all_by_label("From the next start").count(), 2);
    press(&mut window, "Details", 0);
    for criterion in ["Words", "What shows", "Where things are"] {
        assert!(window.query_by_label(criterion).is_some(), "the modes are compared on {criterion}");
    }
    picture(&mut window, "settings-modes-expert-en-dark");

    // What left Settings stands where it acts: paying beside the history, updates over the boards.
    window.state_mut().open("page.wallet");
    window.run_steps(SETTLE_STEPS);
    for moved in ["When you pay", "Consolidate UTXOs", "Sign the old way"] {
        assert!(window.query_all_by_label(moved).count() > 0, "{moved} is on the Wallet page");
    }
    picture(&mut window, "wallet-paying-expert-en-dark");
    window.state_mut().open("page.boards");
    window.run_steps(SETTLE_STEPS);
    assert_eq!(window.query_all_by_label("Update boards on their own").count(), 0, "with no board there is nothing to update");
    let staged = with_boards(&window.state().snapshot());
    window.state_mut().set_stage(staged);
    window.run_steps(SETTLE_STEPS);
    assert!(window.query_all_by_label("Update boards on their own").count() > 0, "board updates stand over the boards");
    picture(&mut window, "boards-updates-expert-en-dark");
}
