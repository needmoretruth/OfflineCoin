//! One speed limit for everything OLC Core mines with on this computer, the Lab included: this
//! app's miner, a miner added beside it and the Lab's miners share it, and the Lab's token panel
//! says the part each of its miners gets. Every speed is the one the nodes measured themselves.
use super::lab_net::LabRun;
use super::live_swap::press;
use super::{node_program, quiet_settings, wait_until, Window, SETTLE_STEPS};
use crate::app::App;
use crate::i18n::{Lang, Mode, Voice};
use crate::local::lab_net::Kind;
use crate::pages::lab_tokens::number;
use std::time::{Duration, Instant};

/// The limit the person set. Two Lab miners at their usual 8,000 each ask twice of it alone.
const LIMIT: u64 = 8_000;
/// A node reads its speed once a second, and the cap hands out a second's hashes in hundredths,
/// so one reading may run a little over while the mean holds; as the budget's own scene allows.
const READING_SLACK: f64 = 1.10;
const MEAN_SLACK: f64 = 1.03;
/// The minute the sum is watched for.
const WATCH_SECS: u64 = 60;

/// Every speed on this computer together, as the nodes last measured it: this app's, the added
/// miners' and the Lab's miners'.
fn together(app: &App) -> u64 {
    let snap = app.snapshot();
    let own = if snap.node.mining { snap.node.hashrate } else { 0 };
    let added: u64 = snap.local.iter().filter_map(|view| view.status.as_ref()).filter(|status| status.mining).map(|status| status.hashrate).sum();
    own + added + lab_hashing(app)
}

fn lab_hashing(app: &App) -> u64 {
    let state = app.lab_state().unwrap_or_default();
    state.members.iter().filter(|member| member.kind.mines()).filter_map(|member| member.status.as_ref()).map(|status| status.hashrate).sum()
}

fn lab_miners_hashing(app: &App) -> usize {
    let state = app.lab_state().unwrap_or_default();
    state.members.iter().filter(|member| member.kind.mines() && member.status.as_ref().is_some_and(|status| status.hashrate > 0)).count()
}

/// The highest and the mean of the sum, read twice a second for `seconds`.
fn watch(window: &mut Window, seconds: u64) -> (u64, u64, usize) {
    let started = Instant::now();
    let mut readings = Vec::new();
    while started.elapsed() < Duration::from_secs(seconds) {
        window.run_steps(1);
        readings.push(together(window.state()));
        std::thread::sleep(Duration::from_millis(500));
    }
    let most = readings.iter().copied().max().unwrap_or(0);
    (most, readings.iter().sum::<u64>() / readings.len().max(1) as u64, readings.len())
}

/// The person set 8,000 H/s, this app mines, one miner was added on Mining, and then two Lab
/// miners were added by their button. For a minute all of them together stay inside 8,000, and
/// the panel of a Lab miner says its part, in basic Korean and in expert English.
#[test]
#[ignore]
fn the_labs_miners_share_the_one_speed_limit_with_this_app_and_its_miners() {
    let basic = Voice { lang: Lang::Ko, mode: Mode::Basic };
    let mut settings = quiet_settings();
    settings.mine_on_start = true;
    settings.hash_cap = LIMIT;
    let mut run = LabRun::start_in("lab-budget", basic, settings, Some(node_program()));
    wait_until(&mut run.window, |app| app.snapshot().node.mining && app.snapshot().full_rate > 0);
    run.window.state_mut().open("page.mining");
    run.window.run_steps(SETTLE_STEPS);
    press(&mut run.window, "채굴기 더하기");
    wait_until(&mut run.window, |app| app.snapshot().local.iter().any(|view| view.status.as_ref().is_some_and(|status| status.mining && status.hashrate > 0)));
    run.window.state_mut().open("page.lab");
    run.window.run_steps(SETTLE_STEPS);
    run.add(Kind::Miner);
    run.add(Kind::Miner);
    run.wait(180, "two Lab miners hash", |run| lab_miners_hashing(run.window.state()) == 2);
    // A new node's first readings come before its part reached it; the minute starts once every
    // speed was read under its part.
    run.pass(5);
    let (most, mean, readings) = watch(&mut run.window, WATCH_SECS);
    let snap = run.window.state().snapshot();
    println!(
        "this app {} H/s, added {:?}, Lab {} H/s: together highest {most}, mean {mean} over {readings} readings against {LIMIT}",
        snap.node.hashrate,
        snap.local.iter().filter_map(|view| view.status.as_ref().map(|status| status.hashrate)).collect::<Vec<_>>(),
        lab_hashing(run.window.state())
    );
    assert!(most as f64 <= LIMIT as f64 * READING_SLACK, "together {most} went over {LIMIT}");
    assert!(mean as f64 <= LIMIT as f64 * MEAN_SLACK, "together {mean} on average, over {LIMIT}");

    for voice in [basic, Voice { lang: Lang::En, mode: Mode::Expert }] {
        run.speak(voice);
        let first = run.named(Kind::Miner, 1);
        run.select(&first);
        let state = run.window.state().lab_state().unwrap_or_default();
        let miner = state.members.iter().find(|member| member.kind == Kind::Miner && member.number == 1).expect("the first Lab miner");
        let (set, part) = (number(miner.speed), number(miner.limit));
        // The bold figure is the speed it mines at now; the speed set stands under it, smaller.
        let template = run.say("net.speed_hash");
        let (before, after) = template.split_once("{}").unwrap_or((template, ""));
        let labels = run.labels();
        let speeds: Vec<u64> =
            labels.iter().filter_map(|label| label.strip_prefix(before)?.strip_suffix(after)?.replace(',', "").parse().ok()).collect();
        let said = run.lines_with("net.set_capped");
        println!("{voice:?}: set {set}, part {part}, speeds shown {speeds:?}, under it {said:?}");
        assert!(miner.limit > 0 && miner.limit < LIMIT / 2, "a Lab miner's part {} is a share of {LIMIT}", miner.limit);
        assert!(!labels.contains(&template.replace("{}", &set)), "the speed in bold is the speed set, {set}, not what it mines at");
        assert!(speeds.iter().any(|speed| *speed > 0 && *speed <= miner.limit * 11 / 10), "the speed in bold is what it mines at: {speeds:?}");
        assert!(said.iter().any(|line| line.contains(&set) && line.contains(&part)), "the speed set and the part under it: {said:?}");
        run.shoot("budget-part");
        run.select(&first);
    }
    run.close();
}
