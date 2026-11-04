//! The words a Bitcoin Core user types first, in the app's console on a real node: `generate 3`
//! mines three blocks at full speed and `generate` says how far it is, `coins` lists the coins as
//! listunspent does, and `mempool` the payments waiting for a block as getrawmempool does. The
//! pictures go to `.scratch/live/console-shots`, at 1280 by 800, dark, in basic Korean and expert
//! English.
use super::live::{live_folder, live_root, live_settings};
use super::live_swap::{press_till, wait_in};
use super::shots::shot;
use super::{app, harness, port_of_scene, Window};
use crate::core::{ChainOrder, Command};
use crate::i18n::{Lang, Mode};
use egui::accesskit::Role;
use egui_kittest::kittest::{By, NodeT, Queryable};
use olc_store::coin_file::{new_coin, NewCoin};
use std::time::{Duration, Instant};

/// Types `line` into the console and runs it.
fn say(window: &mut Window, line: &str) {
    let field = || By::new().role(Role::TextInput);
    window.query(field()).expect("the console's line").focus();
    window.run_steps(2);
    window.query(field()).expect("the console's line").type_text(line);
    window.run_steps(1);
    // The line empties once it runs.
    let ran = |window: &Window| window.query(field()).and_then(|node| node.accesskit_node().value()).is_some_and(|left| left.is_empty());
    press_till(window, "실행", ran);
}

/// Until the console itself says `text`: the notice in the corner may say it a moment earlier.
fn until_shown(window: &mut Window, text: &str) {
    let started = Instant::now();
    let said = |window: &Window| window.query_all(By::new().label_contains(text)).next().is_some();
    let busy = |window: &Window| window.query_all(By::new().label("처리 중")).next().is_some();
    while !said(window) || busy(window) {
        if started.elapsed() > Duration::from_secs(30) {
            shot(window, &live_folder().join("console-shots"), "console-words-stuck");
            panic!("the console never said {text:?}");
        }
        window.run_steps(1);
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn the_console_makes_blocks_and_lists_coins_and_waiting_payments() {
    let now = super::live_swap::unix_ms();
    let values = vec![("name".into(), "Words".into()), ("ticker".into(), "WRD".into()), ("genesis_time_ms".into(), now.to_string())];
    let mut coin = new_coin(&NewCoin { values }).unwrap();
    coin.initial_difficulty = 2_000;
    coin.rules.min_difficulty = 1;
    coin.rules.coinbase_maturity = 1;
    let mut settings = live_settings(port_of_scene(55), &[]);
    settings.mine_on_start = false;
    let mut window = harness(app(live_root("CONSOLE-WORDS"), settings, None), (1280, 800));
    window.state().send(Command::Join { coin: Box::new(coin), address: None });
    wait_in(&mut window, "the coin runs", 2, |snap| snap.node.coin.as_ref().is_some_and(|coin| coin.ticker == "WRD"));
    window.state_mut().open("page.console");
    window.run_steps(4);

    say(&mut window, "generate 3");
    until_shown(&mut window, "Making 3 blocks at full speed");
    let started = Instant::now();
    loop {
        say(&mut window, "generate");
        if window.query_all(By::new().label_contains("3 of 3 blocks made.")).next().is_some() {
            break;
        }
        assert!(started.elapsed() < Duration::from_secs(60), "the three blocks were never made");
        std::thread::sleep(Duration::from_millis(500));
    }
    // Mining rests as it was before: no more than the three asked for.
    std::thread::sleep(Duration::from_secs(2));
    window.run_steps(2);
    assert_eq!(window.state().snapshot().node.height, 3);
    say(&mut window, "coins");
    until_shown(&mut window, "3 coins, 150 in all.");
    say(&mut window, "mempool");
    until_shown(&mut window, "No payments are waiting for a block.");
    // The console prints the answer; no notice floats it again over the line and its Run button.
    window.run_steps(4);
    let times = window.query_all(By::new().label_contains("No payments are waiting")).count();
    assert_eq!(times, 1, "a typed word's answer shows once, in the console");
    let folder = live_folder().join("console-shots");
    shot(&mut window, &folder, "console-words-ko-basic");
    let settings = window.state_mut().settings_mut();
    settings.mode = Mode::Expert;
    settings.language = Lang::En;
    window.run_steps(4);
    shot(&mut window, &folder, "console-words-en-expert");

    // A notice not asked for in the console, as from a button elsewhere, floats above the line
    // and leaves Run free.
    let settings = window.state_mut().settings_mut();
    settings.mode = Mode::Basic;
    settings.language = Lang::Ko;
    window.run_steps(4);
    window.state().send(Command::Chain(ChainOrder::MakeStop));
    let made = By::new().label_contains("지금 만드는 블록이 없습니다");
    let started = Instant::now();
    while window.query_all(made.clone()).next().is_none() {
        assert!(started.elapsed() < Duration::from_secs(30), "no notice came");
        window.run_steps(1);
        std::thread::sleep(Duration::from_millis(100));
    }
    let box_of = |found: Option<egui_kittest::Node<'_>>| found.and_then(|node| node.accesskit_node().bounding_box()).expect("on the screen");
    let notice = box_of(window.query_all(made).next());
    let line = box_of(window.query(By::new().role(Role::TextInput)));
    assert!(notice.y1 < line.y0, "the notice covers the console's line: {notice:?} over {line:?}");
    shot(&mut window, &folder, "console-notice-above-ko-basic");
}
