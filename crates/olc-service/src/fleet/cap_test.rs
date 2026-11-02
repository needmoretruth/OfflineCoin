//! One board's hash cap, measured on the board itself. It runs with one board, from the
//! experiments page, and puts the board back whatever happens.
use olc_board::journal::Journal;
use olc_board::{Args, Board, BoardLink, Extra, Say};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::sleep;
use std::time::Duration;

/// Below the default cap, so a board at the default can still be lowered.
const LOWEST_CAP: u64 = 250;
/// The board's own rate needs this long to forget the old cap.
const SETTLE: Duration = Duration::from_secs(12);
const SAMPLES: u32 = 8;
const BETWEEN: Duration = Duration::from_secs(2);
const TICK: Duration = Duration::from_millis(200);

pub struct CapTest;

/// Half of what the board really does now. A cap above that rate says nothing, and a board at a
/// slow clock stays far below its cap.
fn lower_cap(limit: u64, hashrate: u64) -> u64 {
    let now = match (limit, hashrate) {
        (0, rate) | (rate, 0) => rate,
        (limit, rate) => limit.min(rate),
    };
    (now / 2).max(LOWEST_CAP)
}

/// Waits, and tells whether someone pressed stop meanwhile.
fn wait(time: Duration, stop: &AtomicBool) -> bool {
    let mut left = time;
    while left > Duration::ZERO {
        if stop.load(Ordering::Relaxed) {
            return true;
        }
        sleep(TICK.min(left));
        left = left.saturating_sub(TICK);
    }
    stop.load(Ordering::Relaxed)
}

fn measure(board: &mut Board, saved_limit: u64, say: &Say, stop: &AtomicBool) -> Result<(), String> {
    board.set_mining(true)?;
    if wait(SETTLE, stop) {
        return Ok(());
    }
    let cap = lower_cap(saved_limit, board.status()?.hashrate);
    board.set_limit(cap)?;
    say(format!("1. Cap lowered to {cap} H/s"));
    if wait(SETTLE, stop) {
        return Ok(());
    }
    let mut total = 0;
    for _ in 0..SAMPLES {
        if wait(BETWEEN, stop) {
            return Ok(());
        }
        total += board.status()?.hashrate;
    }
    let measured = total / u64::from(SAMPLES);
    let share = measured * 100 / cap;
    say(format!("2. Measured {measured} H/s, {share}% of the cap"));
    Ok(())
}

impl Extra for CapTest {
    fn command(&self) -> &'static str {
        "captest"
    }

    fn usage(&self) -> &'static str {
        ""
    }

    fn run(&self, _args: &Args) -> Result<(), String> {
        Err("This runs from the app.".into())
    }

    fn script(&self) -> &'static str {
        ""
    }

    fn run_on(
        &self,
        boards: Vec<Box<dyn BoardLink>>,
        _args: &Args,
        say: Say,
        stop: Arc<AtomicBool>,
    ) -> Result<(), String> {
        let Some(link) = boards.into_iter().next() else {
            return Err("It needs a board.".into());
        };
        let mut board = Board::new("tested", link);
        let saved = board.status()?;
        let journal = Journal::standard()?;
        journal.remember(&saved.address, saved.restore_lines())?;
        let result = measure(&mut board, saved.limit, &say, &stop);
        let back = if saved.limit_chosen {
            board.set_limit(saved.limit)
        } else {
            board.set("limit default", |now| !now.limit_chosen)
        };
        let back = back.and_then(|()| board.set_mining(saved.mining));
        if back.is_ok() {
            journal.forget(&saved.address);
            say("3. Cap put back".into());
        }
        result.and(back)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_test_cap_is_one_the_board_can_reach() {
        assert_eq!(lower_cap(15_000, 15_000), 7_500);
        assert_eq!(lower_cap(15_000, 5_000), 2_500, "a board at 80 MHz");
        assert_eq!(lower_cap(15_000, 0), 7_500);
        assert_eq!(lower_cap(0, 24_000), 12_000);
        assert_eq!(lower_cap(1_000, 1_000), 500);
        assert_eq!(lower_cap(300, 300), LOWEST_CAP);
    }
}
