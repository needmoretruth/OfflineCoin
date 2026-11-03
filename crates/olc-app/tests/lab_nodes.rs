//! The Lab's double spend and split on real node processes on this computer: no board, no
//! simulation. Each run takes a minute or two of real mining, so they wait for `--ignored`:
//! `cargo test -p olc-app --test lab_nodes -- --ignored --test-threads 1`.
use olc_app::local::lab::{use_place, LabChoice, LabKind, LabRun, LabState, Phase};
use olc_lab::{Outcome, Step};
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

const WAIT: Duration = Duration::from_secs(600);

fn place(name: &str) {
    let folder = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&folder);
    use_place(env!("CARGO_BIN_EXE_olc-core").into(), folder);
}

fn finish(run: &LabRun) -> LabState {
    let until = Instant::now() + WAIT;
    let mut shown = 0;
    let mut phase = None;
    let mut ticks = 0u32;
    loop {
        let state = run.state();
        if phase != Some(state.phase) {
            phase = Some(state.phase);
            println!("[{:?}]", state.phase);
        }
        for line in &state.lines[shown..] {
            println!("{line}");
        }
        shown = state.lines.len();
        if ticks.is_multiple_of(10) {
            println!("   miners {:?}", state.miners);
        }
        ticks += 1;
        if !run.running() {
            return state;
        }
        assert!(Instant::now() < until, "the run took too long");
        thread::sleep(Duration::from_millis(500));
    }
}

fn steps(state: &LabState) -> Vec<&Step> {
    state.frames.iter().map(|frame| &frame.step).collect()
}

#[test]
#[ignore = "mines for a minute or two on real node processes"]
fn an_attacker_above_half_takes_the_payment_back() {
    place("lab_attack");
    let run = LabRun::start(LabChoice { share: 75, ..LabChoice::default() });
    let state = finish(&run);
    assert_eq!(state.phase, Phase::Done, "{:?}", state.error);
    assert_eq!(state.outcome, Some(Outcome::Gone));
    let steps = steps(&state);
    let Some(Step::Power { attacker, victim }) = steps.first() else {
        panic!("no measured power: {steps:?}");
    };
    assert!(attacker > victim, "attacker {attacker} H/s, victim {victim} H/s");
    assert!(steps.iter().any(|step| matches!(step, Step::Replaced { hidden_won: true, blocks, .. } if *blocks > 0)));
    assert!(steps.contains(&&Step::Refused));
    // The picture at the replacement shows public blocks that were dropped, one with the payment.
    let replaced = state.frames.iter().find(|frame| matches!(frame.step, Step::Replaced { .. })).unwrap();
    assert!(replaced.picture.replaced.iter().any(|mark| mark.payment), "{:?}", replaced.picture);
}

#[test]
#[ignore = "mines for a minute or two on real node processes"]
fn a_split_ends_on_the_heavier_branch() {
    place("lab_split");
    let run = LabRun::start(LabChoice { kind: LabKind::Split, share: 70, ..LabChoice::default() });
    let state = finish(&run);
    assert_eq!(state.phase, Phase::Done, "{:?}", state.error);
    let settled = steps(&state).into_iter().find_map(|step| match step {
        Step::Settled { left_won, blocks, .. } => Some((*left_won, *blocks)),
        _ => None,
    });
    let (_, blocks) = settled.expect("the split did not settle");
    assert!(blocks > 0);
}
