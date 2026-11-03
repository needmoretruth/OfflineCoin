//! A new coin's difficulty rule as a person meets it: three choices named by what each does, and
//! behind Details the blocks each let through after mining changed, on one time axis, then the
//! same criteria for each with where it is used and whether a late block comes easy. Expert mode
//! names them as the trade does, puts the switch for that easy block under the per-block rule, and
//! adds the rows for the half-life, the time rules and Bitcoin's header. With `OLC_SHOTS` it keeps
//! a picture of each.
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

fn opened(lang: Lang, mode: Mode, size: (u32, u32)) -> Window {
    let settings = Settings { language: lang, mode, theme: Theme::Dark, ..quiet_settings() };
    let mut window = harness(app(scratch("coin-rule"), settings, None), size);
    window.state_mut().open("page.coins");
    window.run_steps(SETTLE_STEPS);
    tap(&mut window, "coins.new");
    window.run_steps(SETTLE_STEPS);
    window
}

#[test]
fn a_new_coin_picks_how_it_keeps_its_pace_on_stated_criteria() {
    let mut window = opened(Lang::Ko, Mode::Basic, (1280, 800));
    for choice in ["10블록마다 맞추기", "10블록마다 맞추고 늦으면 쉬운 블록", "블록마다 맞추기"] {
        assert!(window.query_all_by_label(choice).count() > 0, "{choice} is a choice");
    }
    press(&mut window, "자세히");
    assert!(window.query_by_label("채굴이 바뀐 뒤 방식마다 블록이 나온 때").is_some(), "the blocks come first");
    for criterion in ["어디서 쓰나", "늦으면 쉬운 블록", "첫 블록까지", "제 간격으로 돌아오기까지"] {
        assert!(window.query_by_label(criterion).is_some(), "the rules are compared on {criterion}");
    }
    for origin in ["지금의 비트코인", "비트코인 테스트넷", "비트코인 캐시"] {
        assert!(window.query_by_label(origin).is_some(), "where each is used: {origin}");
    }
    // Bitcoin's rule has no easy block; the test network's has, and so has OLC's per-block rule.
    assert_eq!((window.query_all_by_label("없음").count(), window.query_all_by_label("있음").count()), (1, 2));
    picture(&mut window, "coin-rule-basic-ko-dark");
    press(&mut window, "10블록마다 맞추고 늦으면 쉬운 블록");
    picture(&mut window, "coin-rule-picked-basic-ko-dark");

    let mut window = opened(Lang::En, Mode::Expert, (1280, 800));
    for choice in ["Bitcoin", "BIP 94", "ASERT"] {
        assert!(window.query_all_by_label(choice).count() > 0, "{choice} is a choice");
    }
    let late = "Allow minimum-difficulty blocks";
    for row in [late, "ASERT half-life (blocks)", "Median time past (blocks)", "Max time ahead (ms)", "Bitcoin header"] {
        assert!(window.query_all_by_label(row).count() > 0, "expert mode has {row}");
    }
    picture(&mut window, "coin-rule-rows-expert-en-dark");
    press(&mut window, "Details");
    assert!(window.query_by_label("Every 10 blocks; minimum difficulty after 2 block times").is_some());
    assert!(window.query_by_label("Bitcoin testnet4").is_some(), "where each is used");
    assert!(window.query_by_label("Minimum-difficulty blocks").is_some(), "the late floor is compared too");
    picture(&mut window, "coin-rule-expert-en-dark");
    // BIP 94 always lets a late block come at the floor, so its switch would change nothing.
    press(&mut window, "BIP 94");
    assert!(window.query_all_by_label(late).next().is_none(), "the switch stands only under ASERT");

    // A taller window holds the whole comparison and the line under it.
    let mut window = opened(Lang::Ko, Mode::Basic, (1280, 1400));
    press(&mut window, "자세히");
    picture(&mut window, "coin-rule-whole-basic-ko-dark");
}
