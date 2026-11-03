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
    // Three changes were measured; after mining fell to a twentieth, Bitcoin's rule was not back
    // in the two hours watched.
    for case in [DROP20, DROP50, RISE50] {
        assert!(window.query_all_by_label(case).count() > 0, "{case} is a change to look at");
    }
    assert!(window.query_by_label("2시간 안에 못 돌아옴").is_some(), "a rule not back says so");
    // With no easy block the first one is expected at twenty block times; one run's luck is under it.
    assert!(window.query_by_label("보통 10분").is_some() && window.query_by_label("이번에 1.0분").is_some(), "the wait to expect comes first");
    picture(&mut window, "coin-rule-basic-ko-dark");
    press(&mut window, "10블록마다 맞추고 늦으면 쉬운 블록");
    picture(&mut window, "coin-rule-picked-basic-ko-dark");

    let mut window = opened(Lang::En, Mode::Expert, (1280, 800));
    for choice in ["Bitcoin", "BIP 94", "ASERT"] {
        assert!(window.query_all_by_label(choice).count() > 0, "{choice} is a choice");
    }
    let late = "Min-difficulty blocks";
    for row in [late, "ASERT half-life (blocks)", "Median time past (blocks)", "Max time ahead (ms)", "Bitcoin header"] {
        assert!(window.query_all_by_label(row).count() > 0, "expert mode has {row}");
    }
    // The switch's name stands in two lines at most in the column of names.
    let height = |label: &str| window.query_all_by_label(label).next().map_or(0.0, |node| node.rect().height());
    let (line, name) = (height("Ticker"), height(late));
    assert!(line > 0.0 && name <= line * 2.0 + 1.0, "the late switch's name takes {name} for lines of {line}");
    picture(&mut window, "coin-rule-rows-expert-en-dark");
    press(&mut window, "Details");
    assert!(window.query_by_label("Every 10 blocks; minimum difficulty after 2 block times").is_some());
    assert!(window.query_by_label("Bitcoin testnet4").is_some(), "where each is used");
    assert!(window.query_by_label("Minimum-difficulty blocks").is_some(), "the late floor is compared too");
    assert!(window.query_by_label("Expected 10 min").is_some() && window.query_by_label("Measured 1.0 min").is_some());
    picture(&mut window, "coin-rule-expert-en-dark");
    // BIP 94 always lets a late block come at the floor, so its switch would change nothing.
    press(&mut window, "BIP 94");
    assert!(window.query_all_by_label(late).next().is_none(), "the switch stands only under ASERT");

    // A taller window holds the whole comparison and the line under it, for each change.
    let mut window = opened(Lang::Ko, Mode::Basic, (1280, 1400));
    press(&mut window, "자세히");
    picture(&mut window, "coin-rule-whole-basic-ko-dark");
    for (case, name) in [(DROP50, "drop50"), (RISE50, "rise50")] {
        press(&mut window, case);
        picture(&mut window, &format!("coin-rule-whole-{name}-basic-ko-dark"));
    }
    let mut window = opened(Lang::En, Mode::Expert, (1280, 1400));
    press(&mut window, "Details");
    picture(&mut window, "coin-rule-whole-expert-en-dark");
}

const DROP20: &str = "채굴이 20분의 1로 줄 때";
const DROP50: &str = "채굴이 50분의 1로 줄 때";
const RISE50: &str = "채굴이 50배로 늘 때";
