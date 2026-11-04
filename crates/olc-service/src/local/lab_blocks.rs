//! Makes blocks on the Lab's own coin now: the first miner mines with no cap until its chain is
//! the asked number of blocks longer and every other miner has them too. The miners are kept, so
//! the next run starts from these blocks: an attacker that made them already holds coins to pay.
use super::{ids, miner_rows, picture, prepare_miners, sides_of, update, views, LabChoice, LabRun, LabState, Miners, Phase, KEPT, LOOK};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

/// The most blocks one press makes, as the console's `generate` allows.
pub const MOST_BLOCKS: u64 = 1_000;
/// Long enough for a thousand blocks on one thread of a slow computer.
const MAKE_WAIT: Duration = Duration::from_secs(600);

impl LabRun {
    /// Makes `count` blocks on the Lab's coin with the miners of `choice`, kept ones if they fit.
    pub fn make_blocks(choice: LabChoice, count: u64) -> Self {
        let count = count.clamp(1, MOST_BLOCKS);
        let making = Some((0, count));
        let state = Arc::new(Mutex::new(LabState { phase: Phase::Starting, making, ..LabState::default() }));
        let stop = Arc::new(AtomicBool::new(false));
        let (shared, flag) = (state.clone(), stop.clone());
        thread::spawn(move || {
            let result = prepare_miners(&choice, &flag).and_then(|mut miners| {
                let made = make(&choice, &mut miners, count, &shared, &flag);
                // Mined coins and blocks stay for the next run, whatever happened.
                for id in ids(&miners) {
                    let _ = miners.supervisor.send(id, "board mine off");
                    let _ = miners.supervisor.send(id, "board mine-to 0");
                }
                if let Ok(mut slot) = KEPT.lock() {
                    *slot = Some(miners);
                }
                made
            });
            update(&shared, |state| {
                state.kept = true;
                match result {
                    Ok(()) => state.phase = Phase::Done,
                    Err(error) => {
                        state.phase = Phase::Failed;
                        state.error = Some(error);
                    }
                }
            });
        });
        Self { state, stop }
    }
}

/// Mines on the first miner alone, uncapped, until every miner is `count` blocks further.
fn make(choice: &LabChoice, miners: &mut Miners, count: u64, state: &Mutex<LabState>, stop: &AtomicBool) -> Result<(), String> {
    let all = ids(miners);
    for id in &all {
        miners.supervisor.send(*id, "board mine off")?;
        miners.supervisor.send(*id, "board status")?;
    }
    let first = all.first().copied().ok_or("The Lab has no miner.")?;
    let start = wait_height(miners, stop)?;
    update(state, |state| state.phase = Phase::Running);
    // Rests at the asked height on its own: uncapped, it finds several blocks between two looks.
    miners.supervisor.send(first, &format!("board mine-to {}", start + count))?;
    miners.supervisor.send(first, "board limit 0")?;
    miners.supervisor.send(first, "board mine on")?;
    let until = Instant::now() + MAKE_WAIT;
    loop {
        let seen = views(miners);
        let heights: Vec<u64> = seen.iter().map(|view| view.status.as_ref().map_or(0, |status| status.height)).collect();
        let lead = heights.first().copied().unwrap_or(start);
        if lead >= start + count {
            miners.supervisor.send(first, "board mine off")?;
        }
        let drawn = picture(choice, &seen, &sides_of(choice, &seen));
        let rows = miner_rows(miners);
        update(state, |state| {
            state.making = Some((lead.saturating_sub(start).min(count), count));
            state.live = drawn;
            state.miners = rows;
        });
        if heights.iter().all(|height| *height >= start + count) {
            return Ok(());
        }
        if stop.load(Ordering::Relaxed) {
            return Err("stopped by hand.".into());
        }
        if Instant::now() > until {
            return Err("the blocks took too long.".into());
        }
        thread::sleep(LOOK);
    }
}

/// The height every miner agrees on before mining starts, once each has said its chain.
fn wait_height(miners: &mut Miners, stop: &AtomicBool) -> Result<u64, String> {
    let until = Instant::now() + MAKE_WAIT / 10;
    loop {
        let seen = views(miners);
        let heights: Vec<Option<u64>> = seen.iter().map(|view| view.status.as_ref().map(|status| status.height)).collect();
        if let Some(Some(first)) = heights.first() {
            if heights.iter().all(|height| *height == Some(*first)) {
                return Ok(*first);
            }
        }
        if stop.load(Ordering::Relaxed) {
            return Err("stopped by hand.".into());
        }
        if Instant::now() > until {
            return Err("the miners did not agree on their chain.".into());
        }
        thread::sleep(LOOK);
    }
}
