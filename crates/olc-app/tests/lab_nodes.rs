//! The Lab's network on real node processes on this computer: no board, no simulation. Each
//! participant is its own process on the Lab's coin, on 127.0.0.1. Each test takes a minute or
//! more of real mining, so they wait for `--ignored`:
//! `cargo test -p olc-app --test lab_nodes -- --ignored --test-threads 1`.
use olc_app::local::is_running;
use olc_app::local::lab::{use_place, Phase};
use olc_app::local::lab_net::{Kind, LabNet, Member, NetState, Order, Scene, Share, Trick, What};
use olc_app::local::Budget;
use std::collections::BTreeSet;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::sync::{Arc, Mutex};
use olc_lab::{Outcome, Step};
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

fn place(name: &str) {
    let folder = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&folder);
    use_place(env!("CARGO_BIN_EXE_olc-core").into(), folder);
}

/// Waits up to `limit` for `seen`, saying what the network does every few seconds.
fn wait_for(net: &LabNet, limit: Duration, what: &str, seen: impl Fn(&NetState) -> bool) -> NetState {
    let until = Instant::now() + limit;
    let mut said = Instant::now();
    loop {
        let state = net.state();
        if seen(&state) {
            println!("{what}: after {:.0?}", limit.saturating_sub(until.saturating_duration_since(Instant::now())));
            return state;
        }
        if said.elapsed() > Duration::from_secs(5) {
            said = Instant::now();
            for member in &state.members {
                let status = member.status.as_ref();
                println!(
                    "   {:?}{} h={} rate={} bal={} peers={} waits={:?} ended={:?} lab={:?}",
                    member.kind,
                    member.number,
                    member.height(),
                    status.map_or(0, |status| status.hashrate),
                    status.map_or(0, |status| status.balance),
                    status.map_or(0, |status| status.peers),
                    member.waits,
                    member.ended,
                    status.and_then(|status| status.lab.as_ref()).map(|lab| (lab.waiting_bytes, lab.workers.len(), lab.joined.clone(), lab.accepted)),
                );
            }
            if let Some(experiment) = &state.experiment {
                println!("   experiment {:?} {:?}", experiment.phase, experiment.lines.last());
            }
            if let Some(error) = &state.error {
                println!("   error {error}");
            }
        }
        assert!(Instant::now() < until, "{what}: not within {limit:?}");
        thread::sleep(Duration::from_millis(250));
    }
}

fn of(state: &NetState, kind: Kind) -> Vec<Member> {
    state.members.iter().filter(|member| member.kind == kind).cloned().collect()
}

fn pids(state: &NetState) -> Vec<u32> {
    state.members.iter().filter_map(|member| member.pid).collect()
}

fn gone(pids: &[u32]) {
    let until = Instant::now() + Duration::from_secs(10);
    while pids.iter().any(|pid| is_running(*pid)) {
        assert!(Instant::now() < until, "a node process outlived its network");
        thread::sleep(Duration::from_millis(200));
    }
}

fn found_by(state: &NetState, id: u32) -> usize {
    state.events.iter().filter(|event| matches!(event.what, What::Block { finder: Some(finder), .. } if finder == id)).count()
}

#[test]
#[ignore]
fn three_miners_each_find_blocks_of_their_own() {
    place("net-race");
    let net = LabNet::start();
    for _ in 0..3 {
        net.order(Order::Add(Kind::Miner));
    }
    let state = wait_for(&net, Duration::from_secs(120), "each miner found a block", |state| {
        let miners = of(state, Kind::Miner);
        miners.len() == 3 && miners.iter().all(|miner| found_by(state, miner.id) >= 1)
    });
    for miner in of(&state, Kind::Miner) {
        assert!(miner.status.as_ref().is_some_and(|status| status.blocks_mined >= 1));
    }
    let all = pids(&state);
    drop(net);
    gone(&all);
}

#[test]
#[ignore]
fn a_pool_takes_shares_and_pays_its_miners() {
    place("net-pool");
    let net = LabNet::start();
    net.order(Order::Scene(Scene::Pool));
    let state = wait_for(&net, Duration::from_secs(180), "shares and a payout", |state| {
        let shares = |id| state.events.iter().any(|event| matches!(event.what, What::Shares { from, .. } if from == id));
        let paid = |id| state.events.iter().any(|event| matches!(event.what, What::Payout { to, .. } if to == id));
        let joined: Vec<Member> = of(state, Kind::Miner).into_iter().filter(|miner| miner.pool.is_some()).collect();
        joined.len() == 2 && joined.iter().all(|miner| shares(miner.id) && paid(miner.id))
    });
    let pool = &of(&state, Kind::Pool)[0];
    let line = pool.status.as_ref().and_then(|status| status.lab.clone()).unwrap_or_default();
    assert_eq!(line.workers.len(), 2, "two miners work in the pool");
    assert!(line.pool_found >= 1, "the pool found a block");
}

/// The part of a pool's work that a new block may end before the miners move to the new job.
const MOST_STALE: f64 = 0.05;
const STALE_WATCH: Duration = Duration::from_secs(60);

/// Blocks come about two seconds apart in the Lab, so every moment a pool takes to send the new
/// job, and a miner to move to it, is work lost: its shares are of a job a new block ended. The
/// pool sends the new job at once and each miner moves at once, and both count those shares apart
/// as stale.
#[test]
#[ignore]
fn a_pools_miners_lose_little_work_to_jobs_a_new_block_ended() {
    place("net-stale");
    let net = LabNet::start();
    net.order(Order::Scene(Scene::Pool));
    let line = |member: &Member| member.status.as_ref().and_then(|status| status.lab.clone()).unwrap_or_default();
    let joined = |state: &NetState| -> Vec<Member> { of(state, Kind::Miner).into_iter().filter(|miner| miner.pool.is_some()).collect() };
    let work = |state: &NetState| joined(state).iter().map(line).fold((0, 0), |(taken, stale), line| (taken + line.accepted, stale + line.stale));
    let height = |state: &NetState| state.members.iter().map(Member::height).max().unwrap_or(0);
    let start = wait_for(&net, Duration::from_secs(180), "two miners send shares to the pool", |state| {
        let miners = joined(state);
        miners.len() == 2 && miners.iter().map(line).all(|line| line.working && line.accepted > 0)
    });
    let ((taken_before, stale_before), height_before) = (work(&start), height(&start));
    let began = Instant::now();
    thread::sleep(STALE_WATCH);
    let end = net.state();
    let (taken, stale) = work(&end);
    let (taken, stale) = (taken - taken_before, stale - stale_before);
    let blocks = height(&end).saturating_sub(height_before);
    let pool_stale: u64 = of(&end, Kind::Pool).iter().flat_map(|pool| line(pool).workers).map(|worker| worker.stale).sum();
    let lost = stale as f64 / (taken + stale).max(1) as f64;
    println!(
        "over {:.0?}: {blocks} blocks, {:.2} s apart; the pool's miners had {taken} shares taken and {stale} stale, {:.1}% lost; the pool counted {pool_stale} stale",
        began.elapsed(),
        began.elapsed().as_secs_f64() / blocks.max(1) as f64,
        lost * 100.0
    );
    assert!(blocks >= 15, "blocks come about two seconds apart: {blocks} in {STALE_WATCH:?}");
    assert!(taken >= 100, "enough shares to judge by: {taken}");
    assert!(lost <= MOST_STALE, "{:.1}% of the pool's work was on jobs a new block ended", lost * 100.0);
    let all = pids(&end);
    drop(net);
    gone(&all);
}

#[test]
#[ignore]
fn full_pruned_and_light_nodes_hold_what_they_say() {
    place("net-hold");
    let net = LabNet::start();
    for kind in [Kind::Miner, Kind::Full, Kind::Pruned, Kind::Light] {
        net.order(Order::Add(kind));
    }
    wait_for(&net, Duration::from_secs(60), "all four started", |state| {
        state.members.len() == 4 && state.members.iter().all(|member| member.height() >= 1)
    });
    net.order(Order::MakeBlocks(320));
    let state = wait_for(&net, Duration::from_secs(240), "320 blocks on every node", |state| {
        state.making.is_none() && state.members.len() == 4 && state.members.iter().all(|member| member.height() >= 320)
    });
    let line = |kind| of(&state, kind)[0].status.clone().unwrap_or_default();
    let (full, pruned, light) = (line(Kind::Full), line(Kind::Pruned), line(Kind::Light));
    let lab = |status: &olc_app::local::ChildStatus| status.lab.clone().unwrap_or_default();
    assert!(lab(&full).oldest_held <= 1, "the full node holds every block");
    assert!(lab(&pruned).oldest_held > 1, "the pruned node let old blocks go");
    assert!(pruned.height - lab(&pruned).oldest_held + 1 >= 288, "and kept the newest 288");
    assert!(lab(&pruned).unspent > 0 && lab(&full).unspent > 0);
    assert!(light.headers.is_some_and(|headers| headers >= light.height), "the light node holds every header");
    assert!(light.chain.is_empty() && lab(&light).unspent == 0, "and no block or coin");
}

#[test]
#[ignore]
fn a_payment_reaches_the_receiver_and_settles() {
    place("net-pay");
    let net = LabNet::start();
    net.order(Order::Scene(Scene::Payment));
    let state = wait_for(&net, Duration::from_secs(150), "a payment settled", |state| {
        let receiver = of(state, Kind::Receiver).first().map(|member| member.id);
        let paid = state.events.iter().any(|event| matches!(event.what, What::Paid { to, .. } if to == receiver));
        let settled = state.events.iter().any(|event| matches!(event.what, What::Settled { at, .. } if Some(at) == receiver));
        paid && settled
    });
    let receiver = &of(&state, Kind::Receiver)[0];
    assert!(receiver.status.as_ref().is_some_and(|status| status.balance > 0));
}

/// What each node measures it hashes now, by member.
fn rate(state: &NetState, id: u32) -> u64 {
    state.members.iter().find(|member| member.id == id).and_then(|member| member.status.as_ref()).map_or(0, |status| status.hashrate)
}

/// The attacker's share of all hashing as the attack measured it, in percent.
fn measured_share(run: &olc_app::local::lab::LabState) -> Option<u64> {
    run.frames.iter().find_map(|frame| match frame.step {
        Step::Power { attacker, victim } => (attacker * 100).checked_div(attacker + victim),
        _ => None,
    })
}

/// The races that ended: attacker, whether it won, its share, how far behind it gave up.
fn races(state: &NetState) -> Vec<(u32, bool, u64, u64)> {
    state
        .events
        .iter()
        .filter_map(|event| match event.what {
            What::Raced { by, won, share, behind } => Some((by, won, share, behind)),
            _ => None,
        })
        .collect()
}

/// A bad miner starts as any miner does, at the same modest speed: its power is what a person
/// gives it. Before, it started at one and a half times all the honest miners together.
#[test]
#[ignore]
fn a_new_bad_miner_starts_at_a_miners_speed() {
    place("net-attacker-speed");
    let net = LabNet::start();
    for kind in [Kind::Miner, Kind::Miner, Kind::Miner, Kind::Attacker] {
        net.order(Order::Add(kind));
    }
    let state = wait_for(&net, Duration::from_secs(90), "four hashing", |state| {
        state.members.len() == 4 && state.members.iter().all(|member| rate(state, member.id) > 0)
    });
    let attacker = &of(&state, Kind::Attacker)[0];
    let miner = &of(&state, Kind::Miner)[0];
    assert_eq!(attacker.speed, miner.speed, "a bad miner starts at a miner's speed");
    let total: u64 = state.members.iter().map(|member| rate(&state, member.id)).sum();
    let share = rate(&state, attacker.id) * 100 / total.max(1);
    println!("bad miner {} H/s of {total} H/s: {share}%", rate(&state, attacker.id));
    assert!((15..=35).contains(&share), "one of four equal miners holds about a quarter, not {share}%");
}

/// The one-click attack is a fair race, not a set win: one bad miner near 60 % of all hashing and
/// one near 30 %. Each attacks in turn with exactly what it has, and each race is told with the
/// share it really had.
#[test]
#[ignore]
fn the_attack_scene_races_a_strong_and_a_weak_bad_miner() {
    place("net-attack");
    let net = LabNet::start();
    net.order(Order::Scene(Scene::Attack));
    let state = wait_for(&net, Duration::from_secs(90), "five running and hashing", |state| {
        state.members.len() == 5 && state.members.iter().filter(|member| member.kind.mines()).all(|member| rate(state, member.id) > 0)
    });
    let total: u64 = state.members.iter().map(|member| rate(&state, member.id)).sum();
    let shares: Vec<u64> = of(&state, Kind::Attacker).iter().map(|member| rate(&state, member.id) * 100 / total.max(1)).collect();
    println!("bad miners hold {shares:?} % of {total} H/s");
    assert_eq!(shares.len(), 2, "two bad miners");
    assert!((50..=70).contains(&shares[0]) && (20..=40).contains(&shares[1]), "near 60 % and near 30 %: {shares:?}");
    let state = wait_for(&net, Duration::from_secs(1200), "both bad miners raced", |state| races(state).len() >= 2);
    let receiver = of(&state, Kind::Receiver)[0].id;
    for (by, won, share, behind) in races(&state) {
        println!("bad miner {by}: {share}% of all hashing, won {won}, gave up {behind} behind");
        assert!(share > 0, "the race says the share it had");
    }
    let undone = state.events.iter().any(|event| matches!(event.what, What::Undone { at, .. } if at == receiver));
    let won = races(&state).iter().any(|(_, won, _, _)| *won);
    assert_eq!(undone, won, "a payment goes exactly when a hidden chain won");
}

/// A pool turned bad attacks with exactly its miners' power: three of four equal miners work in
/// it, so it holds three quarters of all hashing, and the receiver's payment is undone.
#[test]
#[ignore]
fn a_pool_turned_bad_attacks_with_its_miners_power() {
    place("net-bad-pool");
    let net = LabNet::start();
    for kind in [Kind::Miner, Kind::Miner, Kind::Miner, Kind::Miner, Kind::Pool, Kind::Receiver] {
        net.order(Order::Add(kind));
    }
    let state = wait_for(&net, Duration::from_secs(90), "six running", |state| state.members.len() == 6 && state.members.iter().all(|member| member.status.is_some()));
    let pool = of(&state, Kind::Pool)[0].id;
    for miner in of(&state, Kind::Miner).iter().take(3) {
        net.order(Order::Join(miner.id, Some(pool)));
    }
    net.order(Order::Bad(pool, true));
    let state = wait_for(&net, Duration::from_secs(900), "the bad pool's attack ended", |state| {
        state.experiment.as_ref().is_some_and(|run| matches!(run.phase, Phase::Done | Phase::Failed))
    });
    let run = state.experiment.clone().unwrap_or_default();
    for line in &run.lines {
        println!("{line}");
    }
    let share = measured_share(&run).unwrap_or(0);
    assert!(share >= 60, "the pool attacked with its miners' power, {share}%");
    assert_eq!(run.outcome, Some(Outcome::Gone), "{:?}", run.error);
    let receiver = of(&state, Kind::Receiver)[0].id;
    wait_for(&net, Duration::from_secs(30), "the race and the undone payment are told", |state| {
        let told = races(state).iter().any(|(by, won, _, _)| *by == pool && *won);
        told && state.events.iter().any(|event| matches!(event.what, What::Undone { at, .. } if at == receiver))
    });
}

/// What the miners measure they hash together.
fn hashing(state: &NetState) -> u64 {
    state.members.iter().map(|member| rate(state, member.id)).sum()
}

/// The one budget as the app splits it, with no miner but the Lab's beside this app's own part: the
/// real split, without a window. The limit can change while the network runs.
struct Alone {
    limit: Arc<AtomicU64>,
    asks: Mutex<Vec<(u32, u64)>>,
}

impl Share for Alone {
    fn ask(&self, asks: Vec<(u32, u64)>) {
        if let Ok(mut known) = self.asks.lock() {
            *known = asks;
        }
    }

    fn split(&self) -> Budget {
        let asks = self.asks.lock().map(|asks| asks.clone()).unwrap_or_default();
        Budget::share_with_lab(self.limit.load(AtomicOrdering::SeqCst), 0, 1, &[], &asks)
    }
}

/// The person's speed limit holds the Lab's miners together with this app: past their part every
/// miner slows by the same part, so their shares stay what the person chose, and without a limit
/// each hashes at its own speed.
#[test]
#[ignore]
fn the_lab_keeps_its_miners_under_the_persons_limit() {
    place("net-budget");
    let limit = Arc::new(AtomicU64::new(12_000));
    let net = LabNet::start_with(Alone { limit: limit.clone(), asks: Mutex::new(Vec::new()) });
    for _ in 0..3 {
        net.order(Order::Add(Kind::Miner));
    }
    // Three Lab miners and this app share 12,000 in four parts of 3,000.
    let limits = |state: &NetState| state.members.iter().map(|member| member.limit).collect::<Vec<_>>();
    let state = wait_for(&net, Duration::from_secs(60), "three miners at 3,000 under 12,000", |state| {
        limits(state) == vec![3_000; 3] && (8_500..=9_450).contains(&hashing(state))
    });
    println!("three miners set to 8,000 hash {} H/s under a limit of 12,000", hashing(&state));
    limit.store(6_000, AtomicOrdering::SeqCst);
    let state = wait_for(&net, Duration::from_secs(60), "each slowed to 1,500 under 6,000", |state| {
        limits(state) == vec![1_500; 3] && (4_200..=4_750).contains(&hashing(state))
    });
    println!("under 6,000 they hash {} H/s", hashing(&state));
    assert!(state.members.iter().all(|member| member.speed == 8_000), "each keeps the speed the person chose");
    limit.store(0, AtomicOrdering::SeqCst);
    let state = wait_for(&net, Duration::from_secs(60), "each at its own 8,000 with no limit", |state| {
        limits(state) == vec![8_000; 3] && hashing(state) > 20_000
    });
    println!("with no limit they hash {} H/s", hashing(&state));
}

/// The owner's scene as a person builds it by hand: three miners, two of them in a pool, a sender
/// paying a receiver, then a bad miner. Its attack has to undo the receiver's payment there too.
#[test]
#[ignore]
fn the_bad_miner_undoes_a_payment_in_a_network_built_by_hand() {
    place("net-attack-by-hand");
    let net = LabNet::start();
    for kind in [Kind::Miner, Kind::Miner, Kind::Miner, Kind::Pool] {
        net.order(Order::Add(kind));
    }
    let state = wait_for(&net, Duration::from_secs(120), "four running", |state| state.members.iter().all(|member| member.status.is_some()) && state.members.len() == 4);
    let pool = of(&state, Kind::Pool)[0].id;
    for miner in of(&state, Kind::Miner).iter().take(2) {
        net.order(Order::Join(miner.id, Some(pool)));
    }
    net.order(Order::Add(Kind::Sender));
    net.order(Order::Add(Kind::Receiver));
    let state = wait_for(&net, Duration::from_secs(300), "a payment settled", |state| {
        state.events.iter().any(|event| matches!(event.what, What::Settled { at, .. } if of(state, Kind::Receiver).iter().any(|receiver| receiver.id == at)))
    });
    // The sender pays as often as it can: many payments come while the attack races, and the
    // receiver must still see the attack's payment undone.
    net.order(Order::Speed(of(&state, Kind::Sender)[0].id, 30));
    let receiver = of(&state, Kind::Receiver)[0].id;
    net.order(Order::Add(Kind::Attacker));
    // It starts at a miner's speed, a quarter of all. The person slides it to the most, 40,000
    // of 64,000 H/s, before it has the coins to attack.
    let state = wait_for(&net, Duration::from_secs(60), "the bad miner is on the map", |state| !of(state, Kind::Attacker).is_empty());
    net.order(Order::Speed(of(&state, Kind::Attacker)[0].id, 40_000));
    let state = wait_for(&net, Duration::from_secs(600), "the attack ended", |state| {
        state.experiment.as_ref().is_some_and(|run| matches!(run.phase, Phase::Done | Phase::Failed))
    });
    let run = state.experiment.clone().unwrap_or_default();
    for line in &run.lines {
        println!("{line}");
    }
    assert_eq!(run.outcome, Some(Outcome::Gone), "{:?}", run.error);
    let state = wait_for(&net, Duration::from_secs(60), "the receiver counts the payment undone", |state| {
        state.members.iter().any(|member| member.id == receiver && member.received.undone >= 1)
    });
    let sent = of(&state, Kind::Sender)[0].received.sent;
    println!("the sender paid {sent} times while the receiver kept the undone payment in view");
    net.order(Order::Clear);
}

#[test]
#[ignore]
fn spam_fills_the_waiting_list_and_the_blocks() {
    place("net-spam");
    let net = LabNet::start();
    net.order(Order::Scene(Scene::Spam));
    let state = wait_for(&net, Duration::from_secs(240), "full blocks and a waiting list", |state| {
        let Some(line) = of(state, Kind::Receiver).first().and_then(|member| member.status.as_ref()?.lab.clone()) else {
            return false;
        };
        let full = line.sizes.iter().rev().take(3).filter(|(_, bytes, _)| *bytes * 10 >= line.max_block_bytes * 9).count();
        line.waiting_bytes > line.max_block_bytes && full >= 2
    });
    let spammer = &of(&state, Kind::Spammer)[0];
    assert!(spammer.status.as_ref().and_then(|status| status.lab.as_ref()).is_some_and(|line| line.spam_sent > 50));
}

#[test]
#[ignore]
fn the_split_scene_runs_the_split_experiment() {
    place("net-split");
    let net = LabNet::start();
    net.order(Order::Scene(Scene::Split));
    let state = wait_for(&net, Duration::from_secs(420), "the split ended", |state| {
        state.experiment.as_ref().is_some_and(|run| matches!(run.phase, Phase::Done | Phase::Failed))
    });
    let run = state.experiment.clone().unwrap_or_default();
    assert_eq!(run.phase, Phase::Done, "{:?}", run.error);
    assert!(run.reached(|step| matches!(step, Step::Settled { .. })));
}

#[test]
#[ignore]
fn the_bug_exploiter_tries_its_tricks_on_every_node() {
    place("net-exploit");
    let net = LabNet::start();
    net.order(Order::Scene(Scene::Exploit));
    // Two miners and the full node judge; the light node checks neither blocks nor payments.
    let judges = 3;
    let state = wait_for(&net, Duration::from_secs(300), "every trick tried on every node", |state| {
        of(state, Kind::Exploiter).first().is_some_and(|member| member.tricks.len() >= Trick::ALL.len() * judges)
    });
    let exploiter = &of(&state, Kind::Exploiter)[0];
    let light = of(&state, Kind::Light)[0].id;
    let by: BTreeSet<u32> = exploiter.tricks.iter().map(|row| row.by).collect();
    assert_eq!(by.len(), judges, "every node but the light one was tried");
    assert!(!by.contains(&light));
    for row in &exploiter.tricks {
        assert!(row.refused && row.expected, "{row:?} was not refused for its own rule");
        println!("{:?} refused by {} for {}", row.trick, row.by, row.rule);
    }
    let flashed = state.events.iter().filter(|event| matches!(event.what, What::Refused { .. })).count();
    assert!(flashed >= exploiter.tricks.len().min(200), "each refusal shows on the map");
}

/// The owner's scene runs the bug exploiter on a busy network: payments, spam and a long chain.
/// Each trick must still be judged by its own rule on every node, none left untried.
#[test]
#[ignore]
fn the_bug_exploiter_is_judged_by_its_own_rule_on_a_busy_network() {
    place("net-exploit-busy");
    let net = LabNet::start();
    for kind in [Kind::Miner, Kind::Miner, Kind::Full, Kind::Sender, Kind::Receiver, Kind::Spammer] {
        net.order(Order::Add(kind));
    }
    wait_for(&net, Duration::from_secs(240), "spam and payments flowing", |state| {
        let spam = of(state, Kind::Spammer).first().and_then(|member| Some(member.status.as_ref()?.lab.as_ref()?.spam_sent)).unwrap_or(0);
        let paid = of(state, Kind::Receiver).first().map_or(0, |member| member.received.payments);
        spam > 100 && paid >= 2
    });
    net.order(Order::Add(Kind::Exploiter));
    let judges = 6;
    let state = wait_for(&net, Duration::from_secs(420), "every trick tried on every node", |state| {
        of(state, Kind::Exploiter).first().is_some_and(|member| member.tricks.len() >= Trick::ALL.len() * judges)
    });
    let exploiter = &of(&state, Kind::Exploiter)[0];
    let mut wrong = Vec::new();
    for row in &exploiter.tricks {
        println!("{:?} on {}: refused {} for {:?}, failed {:?}", row.trick, row.by, row.refused, row.rule, row.failed);
        if !(row.refused && row.expected) {
            wrong.push(format!("{:?} on {}: {:?} {:?}", row.trick, row.by, row.rule, row.failed));
        }
    }
    assert!(wrong.is_empty(), "{} of {} not refused for their own rule: {wrong:#?}", wrong.len(), exploiter.tricks.len());
}

#[test]
#[ignore]
fn clear_all_and_closing_leave_no_process() {
    place("net-clear");
    let net = LabNet::start();
    for kind in [Kind::Miner, Kind::Pool, Kind::Receiver] {
        net.order(Order::Add(kind));
    }
    let state = wait_for(&net, Duration::from_secs(60), "three running", |state| pids(state).len() == 3);
    net.order(Order::Clear);
    gone(&pids(&state));
    wait_for(&net, Duration::from_secs(5), "the map is empty", |state| state.members.is_empty());
    net.order(Order::Add(Kind::Miner));
    let state = wait_for(&net, Duration::from_secs(60), "one running", |state| pids(state).len() == 1);
    let id = state.members[0].id;
    net.order(Order::Pause(id, true));
    wait_for(&net, Duration::from_secs(10), "it paused", |state| {
        state.members.first().is_some_and(|member| member.paused && member.status.as_ref().is_some_and(|status| !status.mining))
    });
    let all = pids(&state);
    drop(net);
    gone(&all);
}
