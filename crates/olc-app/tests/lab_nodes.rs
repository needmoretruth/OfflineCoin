//! The Lab's network on real node processes on this computer: no board, no simulation. Each
//! participant is its own process on the Lab's coin, on 127.0.0.1. Each test takes a minute or
//! more of real mining, so they wait for `--ignored`:
//! `cargo test -p olc-app --test lab_nodes -- --ignored --test-threads 1`.
use olc_app::local::is_running;
use olc_app::local::lab::{use_place, LabKind, Phase};
use olc_app::local::lab_net::{Kind, LabNet, Member, NetState, Order, Scene, What, NOT_YET};
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

#[test]
#[ignore]
fn the_hidden_chain_undoes_the_receivers_payment() {
    place("net-attack");
    let net = LabNet::start();
    net.order(Order::Scene(Scene::Attack));
    let state = wait_for(&net, Duration::from_secs(420), "the attack ended", |state| {
        state.experiment.as_ref().is_some_and(|run| matches!(run.phase, Phase::Done | Phase::Failed))
    });
    let run = state.experiment.clone().unwrap_or_default();
    for line in &run.lines {
        println!("{line}");
    }
    assert_eq!(run.kind, Some(LabKind::Attack));
    assert_eq!(run.outcome, Some(Outcome::Gone), "{:?}", run.error);
    assert!(run.reached(|step| matches!(step, Step::Replaced { .. })));
    let receiver = of(&state, Kind::Receiver)[0].id;
    wait_for(&net, Duration::from_secs(20), "the receiver saw it undone", |state| {
        let undone = state.events.iter().any(|event| matches!(event.what, What::Undone { at, .. } if at == receiver));
        let replaced = state.events.iter().any(|event| matches!(event.what, What::Replaced { at, .. } if at == receiver));
        undone && replaced
    });
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
    let state = wait_for(&net, Duration::from_secs(180), "the tricks were tried", |state| {
        of(state, Kind::Exploiter).first().is_some_and(|member| member.waits == Some("net.tricks_not_yet") || !member.tricks.is_empty())
    });
    let exploiter = &of(&state, Kind::Exploiter)[0];
    if exploiter.waits == Some("net.tricks_not_yet") {
        println!("the tricks say: {NOT_YET}");
    }
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
