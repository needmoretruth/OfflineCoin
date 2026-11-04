//! The Lab makes the blocks a person asks for on its own coin, from its page: real miners on this
//! computer, the first one uncapped, the others following. The pictures go to
//! `.scratch/live/lab-make-shots`, at 1280 by 800, dark, in basic Korean and expert English.
use super::live::live_folder;
use super::live_swap::press;
use super::shots::shot;
use super::{app, harness, node_program, quiet_settings, scratch, Window};
use crate::i18n::{Lang, Mode};
use crate::local::lab::{has_kept, remove_kept, use_place};
use egui::accesskit::Role;
use egui::{Key, Modifiers};
use egui_kittest::kittest::{By, NodeT, Queryable};
use std::time::{Duration, Instant};

fn shown(window: &Window, text: &str) -> bool {
    window.query_all(By::new().label_contains(text)).next().is_some()
}

fn save(window: &mut Window, name: &str) {
    shot(window, &live_folder().join("lab-make-shots"), name);
}

#[test]
#[ignore = "mines on real node processes for a minute: run it by name"]
fn the_lab_makes_the_blocks_asked_for_on_its_own_coin() {
    let root = scratch("lab_make");
    use_place(node_program(), root.join("miners"));
    let mut settings = quiet_settings();
    settings.language = Lang::Ko;
    let mut window = harness(app(root.join("app"), settings, None), (1280, 800));
    window.state_mut().open("page.lab");
    window.run_steps(4);
    if let Some(node) = window.query_all(By::new().label("블록 바로 만들기")).next() {
        node.scroll_to_me();
    }
    window.run_steps(3);
    let field = || By::new().role(Role::SpinButton).label("블록 수");
    let count = window.query(field()).or_else(|| window.query(By::new().role(Role::TextInput).label("블록 수")));
    count.expect("a field for how many blocks").focus();
    window.run_steps(2);
    window.key_press_modifiers(Modifiers::COMMAND, Key::A);
    window.run_steps(1);
    let count = window.query(field()).or_else(|| window.query(By::new().role(Role::TextInput).label("블록 수")));
    count.expect("a field for how many blocks").type_text("5");
    window.run_steps(3);
    press(&mut window, "블록 만들기");

    let started = Instant::now();
    let mut seen_making = false;
    while !shown(&window, "블록 5개를 만들었습니다") {
        seen_making |= shown(&window, "블록 만드는 중");
        assert!(!shown(&window, "블록을 만들지 못했습니다"), "the Lab could not make the blocks");
        assert!(started.elapsed() < Duration::from_secs(180), "the blocks took too long");
        window.run_steps(1);
        std::thread::sleep(Duration::from_millis(200));
    }
    assert!(seen_making, "the page said how far it was while it made them");
    // A fresh Lab coin starts at block 0, and every miner has the five.
    let made = window.query_all(By::new().label_contains("블록 5개를 만들었습니다")).next().and_then(|node| node.accesskit_node().label());
    assert!(shown(&window, "실험용 코인은 지금 블록 5입니다"), "the Lab's coin is not at block 5: {made:?}");
    assert!(has_kept(), "the miners are kept for the next run");
    window.run_steps(4);
    save(&mut window, "lab-make-ko-basic");
    let settings = window.state_mut().settings_mut();
    settings.mode = Mode::Expert;
    settings.language = Lang::En;
    window.run_steps(4);
    if let Some(node) = window.query_all(By::new().label("Make blocks now")).next() {
        node.scroll_to_me();
    }
    window.run_steps(3);
    save(&mut window, "lab-make-en-expert");
    remove_kept();
}
