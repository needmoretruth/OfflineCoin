//! The Lab in the real app, as a person meets it: every kind is one click from its first view in
//! both modes, and so is every setup. The tests that start real node processes click the screen
//! only, wait for `--ignored`, and with `OLC_SHOTS` naming a folder keep each moment as a picture
//! at 1280 by 800: `cargo test -p olc-app --lib lab_shots -- --ignored --test-threads 1`.
use super::lab_net::{LabRun, SHOT_SIZE};
use super::{app, harness, quiet_settings, scratch, SETTLE_STEPS};
use crate::i18n::{self, Lang, Mode, Voice};
use crate::local::lab_net::{Kind, Scene};
use crate::pages::lab_palette::scene_key;
use crate::pages::lab_tokens::kind_key;
use egui_kittest::kittest::Queryable;

const BASIC_KO: Voice = Voice { lang: Lang::Ko, mode: Mode::Basic };
const EXPERT_EN: Voice = Voice { lang: Lang::En, mode: Mode::Expert };

/// Every "+ kind" and every setup is on the Lab's first view, in both modes and languages.
/// Spam payments sent before its pile is the one on the map.
const SPAM_SEEN: f64 = 100.0;
/// Attacks a person may press before the race is won.
const ATTACK_TRIES: usize = 3;

#[test]
fn every_kind_and_setup_is_one_click_from_the_first_view() {
    let mut window = harness(app(scratch("lab_page"), quiet_settings(), None), SHOT_SIZE);
    for mode in [Mode::Basic, Mode::Expert] {
        for lang in [Lang::En, Lang::Ko] {
            window.state_mut().settings_mut().mode = mode;
            window.state_mut().settings_mut().language = lang;
            window.state_mut().open("page.lab");
            window.run_steps(SETTLE_STEPS);
            let voice = Voice { lang, mode };
            for kind in Kind::ALL {
                let label = format!("+ {}", i18n::text(voice, kind_key(kind)));
                assert!(window.query_by_label(&label).is_some(), "no {label} in {mode:?} {lang:?}");
            }
            for scene in Scene::ALL {
                // The boards' own experiment in expert mode may carry the same name.
                let label = i18n::text(voice, scene_key(scene));
                assert!(window.query_all_by_label(label).next().is_some(), "no {label} in {mode:?} {lang:?}");
            }
        }
    }
}

/// Waits until the token's line counts at least `least`.
fn counts(run: &mut LabRun, name: &str, least: f64, limit: u64) {
    let what = format!("{name} counts {least}");
    run.wait(limit, &what, |run| run.count(name).is_some_and(|count| count >= least));
}

/// Adds one of `kind` by its button, waits until its token says more than "starting", and keeps
/// the picture of it just added.
fn add_and_see(run: &mut LabRun, kind: Kind, number: u32) -> String {
    let before = run.clicks;
    run.add(kind);
    assert_eq!(run.clicks - before, 1, "{kind:?} takes one click");
    let name = run.named(kind, number);
    let starting = format!("{name}, {}", run.say("net.wait_start"));
    run.wait(60, &format!("{name} started"), |run| run.token(&name).is_some_and(|label| label != starting));
    if number == 1 {
        run.shoot(&format!("kind-{}", format!("{kind:?}").to_lowercase()));
    }
    name
}

/// The bad miner races with the speed it has. Under half it usually loses and the line under the
/// map says why; the person then slides its speed to the most and attacks again, until the
/// receiver's payment is undone. Pictures: the race, what was told, the strong race, the end.
fn attack_until_undone(run: &mut LabRun, attacker: &str, receiver: &str) {
    let (stop, again, speed) = (run.say("lab2.stop"), run.say("net.again_attack"), run.say("net.speed"));
    let undone = |run: &LabRun| receiver_counts(run).is_some_and(|(_, _, undone)| undone >= 1.0) && run.line_numbers("net.say_undone").is_some();
    let told = |run: &LabRun| run.line_numbers("net.say_won").is_some() || run.line_numbers("net.say_lost").is_some();
    run.wait(240, "the attack starts", |run| run.has_button(stop));
    run.pass(4);
    run.window.get_by_role_and_label(egui::accesskit::Role::Button, stop).scroll_to_me();
    run.shoot("04-attack-race");
    run.wait(600, "the race ends and is told", |run| !run.has_button(stop) && told(run));
    run.shoot("04-attack-told");
    for _ in 0..ATTACK_TRIES {
        if undone(run) {
            break;
        }
        run.select(attacker);
        run.slide(speed, 1.0);
        run.wait(60, "the attack can start again", |run| run.has_button(again));
        // Its share is measured, not set: the token shows it once the node hashes faster.
        run.wait(60, "the bad miner holds more than half", |run| run.count(attacker).is_some_and(|share| share > 50.0));
        run.shoot("04-attack-faster");
        run.press(again);
        run.wait(240, "the attack starts again", |run| run.has_button(stop));
        run.pass(4);
        run.shoot("04-attack-strong-race");
        run.select(receiver);
        run.wait(600, "the receiver's payment is undone, or the attack ended", |run| undone(run) || !run.has_button(stop));
        if !undone(run) {
            // What the run said at its end: the outcome and its steps.
            let more = run.say("explain.details");
            if run.has_button(more) {
                run.press(more);
            }
            println!("attack ended without undoing: {:?}", run.labels());
        }
    }
    assert!(undone(run), "the attack never undid the payment");
    run.shoot("04-attack-undone");
}

/// A pool turns bad by one click: its ring goes red, its panel says the real share of its
/// miners, and once it holds enough coins it races with that share. Turned honest after.
fn bad_pool_races(run: &mut LabRun, pool: &str) {
    run.select(pool);
    let before = run.clicks;
    run.press(run.say("net.turn_bad"));
    assert_eq!(run.clicks - before, 1, "a pool turns bad in one click");
    let bad_pool = format!("{} 1", run.say("net.kind_bad_pool"));
    run.wait(30, "the pool shows its power", |run| run.token(&bad_pool).is_some() && run.line_numbers("net.fact_bad_pool").is_some());
    run.shoot("04-bad-pool");
    let stop = run.say("lab2.stop");
    run.wait(300, "the bad pool's attack starts", |run| run.has_button(stop));
    run.pass(4);
    run.shoot("04-bad-pool-race");
    let told = |run: &LabRun| run.lines_with("net.say_won").into_iter().chain(run.lines_with("net.say_lost")).any(|line| line.contains(&bad_pool));
    run.wait(600, "the bad pool's race is told", |run| !run.has_button(stop) && told(run));
    run.shoot("04-bad-pool-told");
    let honest = run.say("net.turn_honest");
    if !run.has_button(honest) {
        run.select(&bad_pool);
    }
    run.press(honest);
    run.wait(30, "the pool is honest again", |run| run.token(pool).is_some());
    run.select(pool);
}

/// The receiver's panel line: payments received, settled and undone.
/// The spam's own payments, not what an attack sent back to wait, fill more than a block.
fn spam_piled(run: &LabRun, spammer: &str) -> bool {
    run.count(spammer).is_some_and(|sent| sent >= SPAM_SEEN) && run.line_numbers("net.pile_over").is_some()
}

fn receiver_counts(run: &LabRun) -> Option<(f64, f64, f64)> {
    match run.line_numbers("net.fact_receiver")?.as_slice() {
        [got, settled, undone, ..] => Some((*got, *settled, *undone)),
        _ => None,
    }
}

/// The owner's scene, in plain Korean, by clicks only: three miners race; a pool takes two of
/// them by drag; a sender pays a receiver; a bad miner undoes the receiver's payment; spam fills
/// the waiting list; a bug exploiter tries its tricks; then the three keepers; then remove, pause,
/// clear, leaving the page and closing the app leave no process behind.
#[test]
#[ignore = "runs a network of real node processes for minutes: run it by name"]
fn a_person_builds_the_network_by_clicks_in_plain_korean() {
    let mut run = LabRun::start("lab-net-basic", BASIC_KO);
    run.shoot("00-empty");
    // 1. Three miners race, and each block lands on its finder.
    let miners: Vec<String> = (1..=3).map(|number| add_and_see(&mut run, Kind::Miner, number)).collect();
    for miner in &miners {
        counts(&mut run, miner, 1.0, 120);
    }
    run.shoot("01-race");
    // 2. A pool, and two miners dragged into it share.
    let pool = add_and_see(&mut run, Kind::Pool, 1);
    for miner in &miners[..2] {
        let before = run.clicks;
        run.drag(miner, &pool);
        assert_eq!(run.clicks - before, 1, "joining a pool takes one drag");
    }
    counts(&mut run, &pool, 2.0, 30);
    for miner in &miners[..2] {
        counts(&mut run, miner, 1.0, 120);
    }
    run.select(&miners[0]);
    let paid = |run: &LabRun| run.line_numbers("net.fact_in_pool").and_then(|numbers| numbers.last().copied()).is_some_and(|coins| coins > 0.0);
    run.wait(180, "a payout from the pool", paid);
    run.shoot("02-pool-panel");
    run.select(&miners[0]);
    run.shoot("02-pool");
    // 3. A sender pays a receiver: the payment travels, then settles.
    let sender = add_and_see(&mut run, Kind::Sender, 1);
    let receiver = add_and_see(&mut run, Kind::Receiver, 1);
    run.wait(120, "the first payment leaves", |run| run.count(&sender).is_some_and(|sent| sent >= 1.0));
    run.shoot_now("03-payment-travels");
    run.select(&receiver);
    run.wait(120, "a payment settles", |run| receiver_counts(run).is_some_and(|(_, settled, _)| settled >= 1.0));
    run.shoot("03-payment-settled");
    // 4. A bad miner joins at a miner's speed, a quarter of all: its race usually fails and the
    //    line under the map says why. The person slides it up past half and attacks again, and
    //    the receiver's payment is undone.
    let attacker = add_and_see(&mut run, Kind::Attacker, 1);
    attack_until_undone(&mut run, &attacker, &receiver);
    run.select(&receiver);
    // 4b. The pool turns bad with its two miners' power, less than half: its race fails.
    bad_pool_races(&mut run, &pool);
    // 5. Spam: the waiting list grows past what a block holds.
    let spammer = add_and_see(&mut run, Kind::Spammer, 1);
    run.wait(240, "the spam passes a block", |run| spam_piled(run, &spammer));
    run.shoot("05-spam");
    // 6. A bug exploiter: each trick refused by every node, with the rule named.
    let exploiter = add_and_see(&mut run, Kind::Exploiter, 1);
    every_trick_refused(&mut run, &exploiter);
    // The three keepers hold what their lines say.
    for kind in [Kind::Full, Kind::Pruned, Kind::Light] {
        let keeper = add_and_see(&mut run, kind, 1);
        counts(&mut run, &keeper, 1.0, 120);
    }
    run.shoot("07-everyone");
    fits(&mut run, &miners[2]);
    // Pause, remove and clear, by click.
    run.select(&miners[2]);
    run.press(run.say("net.preset_stop"));
    let paused = format!("{}, {}", miners[2], run.say("net.paused"));
    run.wait(20, "the miner paused", |run| run.token(&miners[2]).is_some_and(|label| label == paused));
    run.press(run.say("net.remove"));
    run.wait(20, "the miner was removed", |run| run.token(&miners[2]).is_none());
    run.press(run.say("net.clear"));
    run.none_left("clear all");
    // Leaving the Lab stops what runs there; so does closing the app.
    run.add(Kind::Miner);
    run.wait(60, "a miner runs", |run| run.processes() > 0);
    run.window.state_mut().open("page.home");
    run.window.run_steps(SETTLE_STEPS);
    run.none_left("leaving the Lab");
    run.window.state_mut().open("page.lab");
    run.window.run_steps(SETTLE_STEPS);
    run.add(Kind::Miner);
    run.wait(60, "a miner runs again", |run| run.processes() > 0);
    run.close();
}

/// Every kind by its own button in expert English, each seen doing its part on real processes.
#[test]
#[ignore = "runs a network of real node processes for minutes: run it by name"]
fn every_kind_does_its_part_in_expert_english() {
    let mut run = LabRun::start("lab-net-expert", EXPERT_EN);
    run.shoot("00-empty");
    let miners: Vec<String> = (1..=2).map(|number| add_and_see(&mut run, Kind::Miner, number)).collect();
    for miner in &miners {
        counts(&mut run, miner, 1.0, 120);
    }
    let pool = add_and_see(&mut run, Kind::Pool, 1);
    run.drag(&miners[0], &pool);
    counts(&mut run, &pool, 1.0, 30);
    counts(&mut run, &miners[0], 1.0, 120);
    run.shoot("02-pool");
    // An expert reads the shares a new block ended apart: on the miner, and on the pool's lines.
    run.select(&miners[0]);
    run.wait(60, "the miner says its stale shares of all", |run| {
        run.line_numbers("pro.stale_shares").is_some_and(|numbers| numbers.len() == 2 && numbers[1] > 0.0)
    });
    run.select(&miners[0]);
    run.select(&pool);
    run.wait(30, "the pool says each worker's stale shares", |run| !run.lines_with("pro.worker_stale").is_empty());
    run.shoot("02-pool-stale");
    run.select(&pool);
    for kind in [Kind::Full, Kind::Light] {
        let keeper = add_and_see(&mut run, kind, 1);
        counts(&mut run, &keeper, 1.0, 120);
    }
    // A pruned node lets go of blocks past the newest 288: one click makes the chain that long.
    // These tests mine under a speed limit; the default has none, and Settings lifts it so.
    let pruned = add_and_see(&mut run, Kind::Pruned, 1);
    let limit = std::mem::take(&mut run.window.state_mut().settings_mut().hash_cap);
    run.press("+300");
    run.wait(300, "the pruned node let old blocks go", |run| run.count(&pruned).is_some_and(|below| below >= 1.0));
    run.window.state_mut().settings_mut().hash_cap = limit;
    // Its token and its panel, which names the height it pruned below.
    run.select(&pruned);
    run.shoot("07-pruned");
    run.select(&pruned);
    let sender = add_and_see(&mut run, Kind::Sender, 1);
    let receiver = add_and_see(&mut run, Kind::Receiver, 1);
    run.wait(120, "the first payment leaves", |run| run.count(&sender).is_some_and(|sent| sent >= 1.0));
    run.shoot_now("03-payment-travels");
    counts(&mut run, &receiver, 1.0, 120);
    let spammer = add_and_see(&mut run, Kind::Spammer, 1);
    run.wait(240, "the spam passes a block", |run| spam_piled(run, &spammer));
    // Behind a pile larger than a block the attack's payment would not get in: the bad miner
    // says so and waits, and starts once the person stops the spammer.
    let attacker = add_and_see(&mut run, Kind::Attacker, 1);
    run.select(&attacker);
    let pile = run.say("net.wait_pile");
    run.wait(240, "the bad miner says the pile is in its way", |run| run.labels().iter().any(|label| label == pile));
    run.shoot("04-attack-pile");
    run.select(&spammer);
    run.press(run.say("net.preset_stop"));
    run.select(&receiver);
    attack_until_undone(&mut run, &attacker, &receiver);
    run.select(&receiver);
    bad_pool_races(&mut run, &pool);
    let exploiter = add_and_see(&mut run, Kind::Exploiter, 1);
    every_trick_refused(&mut run, &exploiter);
    fits(&mut run, &miners[1]);
    run.close();
}

/// Every setup by its one button in expert English, each seen to its end on real processes.
#[test]
#[ignore = "runs a network of real node processes for minutes: run it by name"]
fn every_setup_is_one_click_in_expert_english() {
    let mut run = LabRun::start("lab-net-setups", EXPERT_EN);
    let stop = run.say("lab2.stop");
    for scene in Scene::ALL {
        let before = run.clicks;
        run.press(run.say(scene_key(scene)));
        assert_eq!(run.clicks - before, 1, "{scene:?} takes one click");
        let miner = |number| run.named(Kind::Miner, number);
        let (first, second) = (miner(1), miner(2));
        match scene {
            Scene::Race => {
                for number in 1..=3 {
                    let name = run.named(Kind::Miner, number);
                    counts(&mut run, &name, 1.0, 120);
                }
            }
            Scene::Pool => {
                let pool = run.named(Kind::Pool, 1);
                counts(&mut run, &pool, 2.0, 60);
                counts(&mut run, &first, 1.0, 120);
                counts(&mut run, &second, 1.0, 120);
            }
            Scene::Payment => {
                let receiver = run.named(Kind::Receiver, 1);
                counts(&mut run, &receiver, 1.0, 120);
                run.select(&receiver);
                run.wait(120, "a payment settles", |run| receiver_counts(run).is_some_and(|(_, settled, _)| settled >= 1.0));
            }
            // A fair race: the bad miner near 60 % and the one near 30 % each race in turn.
            Scene::Attack => {
                let told = std::cell::RefCell::new(std::collections::BTreeSet::new());
                run.wait(1200, "both bad miners raced", |run| {
                    told.borrow_mut().extend(run.lines_with("net.say_won").into_iter().chain(run.lines_with("net.say_lost")));
                    told.borrow().len() >= 2
                });
                println!("{:#?}", told.borrow());
            }
            Scene::Split => {
                run.wait(240, "the run starts", |run| run.has_button(stop));
                run.wait(600, "the run ends", |run| !run.has_button(stop));
                let failed = run.say("lab2.failed");
                let fixed = failed.split("{}").next().unwrap_or(failed).trim();
                assert!(!run.labels().iter().any(|label| label.starts_with(fixed)), "{scene:?} failed: {:?}", run.labels());
            }
            Scene::Spam => {
                let spammer = run.named(Kind::Spammer, 1);
                run.wait(240, "the spam passes a block", |run| spam_piled(run, &spammer));
            }
            Scene::Exploit => {
                let exploiter = run.named(Kind::Exploiter, 1);
                every_trick_refused(&mut run, &exploiter);
            }
        }
        run.shoot(&format!("setup-{}", format!("{scene:?}").to_lowercase()));
    }
    run.close();
}

/// The first refusal flashes on the map; then every trick has been tried on every node that
/// judges, and every one of them refused it.
fn every_trick_refused(run: &mut LabRun, exploiter: &str) {
    run.wait(240, "the first refusal", |run| run.count(exploiter).is_some_and(|refused| refused >= 1.0));
    run.shoot_now("06-refused");
    run.select(exploiter);
    let all = crate::local::lab_net::Trick::ALL.len() as f64;
    // A try that could not be made is said with the node it was for. Every node answers every
    // trick first, so a missing refusal fails here with its cause, not as a long wait.
    let failed = |run: &LabRun| run.line_numbers("net.trick_failed").and_then(|numbers| numbers.first().copied()).unwrap_or(0.0);
    let answered = |run: &LabRun| match run.line_numbers("net.fact_tricks").as_deref() {
        Some([tried, nodes, refused, ..]) => *tried >= all && *refused + failed(run) >= all * *nodes,
        _ => false,
    };
    run.wait(480, "every node answers every trick", answered);
    assert!(run.lines_with("net.trick_failed").is_empty(), "tricks not tried: {:?}", run.lines_with("net.trick_failed"));
    assert!(run.line_numbers("net.trick_let_through").is_none(), "a trick was let through: {:?}", run.labels());
    println!("every trick refused by every node");
    run.press(run.say("net.tricks_all"));
    run.shoot("06-tricks");
    run.select(exploiter);
}

/// The Lab with everyone in it, the panel open, fits at the smallest and a large window with
/// text at 150 %.
fn fits(run: &mut LabRun, selected: &str) {
    run.select(selected);
    let mut wrong = run.overflow_at((960, 620), 150);
    wrong.extend(run.overflow_at((1920, 1080), 150));
    run.back_to_normal();
    run.shoot("08-panel");
    run.select(selected);
    assert!(wrong.is_empty(), "these reach past the window's edge: {wrong:#?}");
}

/// A Lab left open two hours with nothing mining: a miner added by one click holds back a few
/// seconds to hear its neighbours, as every computer does on a tip over an hour old. Its token
/// says so meanwhile, and it then mines.
#[test]
#[ignore = "runs a real node process: run it by name"]
fn a_miner_added_to_a_lab_left_idle_says_it_waits_and_then_mines() {
    crate::local::lab::next_chain_idle_for(std::time::Duration::from_secs(2 * 60 * 60));
    let mut run = LabRun::start("lab-net-idle", BASIC_KO);
    let (clicked, before) = (std::time::Instant::now(), run.clicks);
    run.add(Kind::Miner);
    assert_eq!(run.clicks - before, 1, "one click");
    let miner = run.named(Kind::Miner, 1);
    let looking = format!("{miner}, {}", run.say("net.tok_looking"));
    run.wait(30, "the miner says it waits for its neighbours", |run| run.token(&miner).is_some_and(|label| label == looking));
    crate::local::lab::next_chain_idle_for(std::time::Duration::ZERO);
    run.shoot("idle-looking");
    counts(&mut run, &miner, 1.0, 40);
    println!("from the click to its first block: {:.0?}", clicked.elapsed());
    run.shoot("idle-mining");
    run.close();
}

/// The same idle Lab with a full node in it: the new miner hears its one neighbour at once and
/// has nothing left to wait for, so its token shows it hashing within five seconds of the click.
#[test]
#[ignore = "runs a real node process: run it by name"]
fn a_miner_added_beside_a_neighbour_on_an_idle_lab_mines_within_seconds() {
    crate::local::lab::next_chain_idle_for(std::time::Duration::from_secs(2 * 60 * 60));
    let mut run = LabRun::start("lab-net-idle-near", EXPERT_EN);
    run.add(Kind::Full);
    let keeper = run.named(Kind::Full, 1);
    let starting = format!("{keeper}, {}", run.say("net.wait_start"));
    run.wait(60, "the full node started", |run| run.token(&keeper).is_some_and(|label| label != starting));
    crate::local::lab::next_chain_idle_for(std::time::Duration::ZERO);
    let clicked = std::time::Instant::now();
    run.add(Kind::Miner);
    let miner = run.named(Kind::Miner, 1);
    // An expert miner's line starts with the speed it measures: above zero, it hashes.
    let hashing = |run: &LabRun| run.token(&miner).and_then(|label| super::lab_net::numbers(&label).get(1).copied()).is_some_and(|speed| speed > 0.0);
    run.wait(5, "the miner hashes", hashing);
    println!("from the click to hashing: {:.1?}", clicked.elapsed());
    // The line above the map counts the two at once, not after its next look at the memory.
    let running = run.line_numbers("net.room").and_then(|numbers| numbers.get(1).copied());
    assert_eq!(running, Some(2.0), "the room line counts both: {:?}", run.labels());
    run.shoot("idle-near-hashing");
    counts(&mut run, &miner, 1.0, 60);
    println!("from the click to its first block: {:.1?}", clicked.elapsed());
    run.close();
}
