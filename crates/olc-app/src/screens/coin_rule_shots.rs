//! A new coin's difficulty rule as a person meets it: three choices named by where they are used,
//! and behind Details the picture of how long each took to get back to its pace, then the same
//! three criteria for each. Expert mode names them as the trade does and adds the rows for the
//! half-life, the time rules and Bitcoin's header. With `OLC_SHOTS` it keeps a picture of each.
use super::shots::{shot, tap};
use super::{app, harness, quiet_settings, scratch, Window, SETTLE_STEPS};
use crate::i18n::{Lang, Mode};
use crate::settings::{Settings, Theme};
use egui_kittest::kittest::Queryable;
use std::path::Path;

fn picture(window: &mut Window, name: &str) {
    if let Ok(folder) = std::env::var("OLC_SHOTS") {
        shot(window, Path::new(&folder), name);
    }
}

fn press(window: &mut Window, label: &str) {
    let found = window.query_all_by_label(label).next();
    found.unwrap_or_else(|| panic!("{label} is not on the screen")).click_accesskit();
    window.run_steps(SETTLE_STEPS);
}

fn opened(lang: Lang, mode: Mode) -> Window {
    let settings = Settings { language: lang, mode, theme: Theme::Dark, ..quiet_settings() };
    let mut window = harness(app(scratch("coin-rule"), settings, None), (1280, 800));
    window.state_mut().open("page.coins");
    window.run_steps(SETTLE_STEPS);
    tap(&mut window, "coins.new");
    window.run_steps(SETTLE_STEPS);
    window
}

#[test]
fn a_new_coin_picks_how_it_keeps_its_pace_on_stated_criteria() {
    let mut window = opened(Lang::Ko, Mode::Basic);
    for choice in ["비트코인", "비트코인 테스트넷", "비트코인 캐시"] {
        assert!(window.query_all_by_label(choice).count() > 0, "{choice} is a choice");
    }
    press(&mut window, "자세히");
    assert!(window.query_by_label("방식마다 제 간격으로 돌아오기까지 걸린 시간").is_some(), "the picture comes first");
    for criterion in ["맞추는 때", "채굴이 줄고 첫 블록", "제 간격으로 돌아오기"] {
        assert!(window.query_by_label(criterion).is_some(), "the rules are compared on {criterion}");
    }
    assert!(window.query_by_label("16.3분, 블록 8개").is_some(), "the measured numbers");
    picture(&mut window, "coin-rule-basic-ko-dark");
    press(&mut window, "비트코인 테스트넷");
    picture(&mut window, "coin-rule-picked-basic-ko-dark");

    let mut window = opened(Lang::En, Mode::Expert);
    for choice in ["Bitcoin", "BIP 94", "ASERT"] {
        assert!(window.query_all_by_label(choice).count() > 0, "{choice} is a choice");
    }
    for row in ["ASERT half-life (blocks)", "Median time past (blocks)", "Max time ahead (ms)", "Bitcoin header"] {
        assert!(window.query_all_by_label(row).count() > 0, "expert mode has {row}");
    }
    press(&mut window, "Details");
    assert!(window.query_by_label("Every 10 blocks; minimum difficulty after 2 block times").is_some());
    picture(&mut window, "coin-rule-expert-en-dark");
}
