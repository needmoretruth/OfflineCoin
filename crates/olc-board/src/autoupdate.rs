//! Keeps every trusted board on the build this program carries.
use crate::manager::Manager;
use olc_control::BoardId;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

const LOOK_EVERY: Duration = Duration::from_secs(5);
/// Tries in a row before a board is left alone for `REST`.
const TRIES: u32 = 3;
const BETWEEN_TRIES: Duration = Duration::from_secs(60);
const REST: Duration = Duration::from_secs(30 * 60);

#[derive(Clone, Copy)]
struct Attempts {
    count: u32,
    last: Instant,
}

impl Attempts {
    fn may_try(&self) -> bool {
        let wait = if self.count >= TRIES {
            REST
        } else {
            BETWEEN_TRIES
        };
        self.last.elapsed() >= wait
    }
}

/// The number in a build such as `b12`, so that `b10` comes after `b9`.
fn number_of(build: &str) -> Option<u32> {
    build.strip_prefix('b')?.parse().ok()
}

pub fn is_older(build: &str, carried: &str) -> bool {
    match (number_of(build), number_of(carried)) {
        (Some(have), Some(new)) => have < new,
        // A build with no number is one somebody made by hand, and it stays.
        _ => false,
    }
}

/// Runs for as long as the program does. `carried` is the build to keep boards on.
pub fn start(manager: Arc<Manager>, carried: &'static str) {
    thread::spawn(move || {
        let mut attempts: BTreeMap<BoardId, Attempts> = BTreeMap::new();
        loop {
            thread::sleep(LOOK_EVERY);
            if manager.is_busy() {
                continue;
            }
            let Some(board) = next_board(&manager, carried, &attempts) else {
                continue;
            };
            let tried = attempts.get(&board).map_or(0, |seen| seen.count % TRIES);
            attempts.insert(
                board,
                Attempts {
                    count: tried + 1,
                    last: Instant::now(),
                },
            );
            if manager.update(board) {
                attempts.remove(&board);
            }
        }
    });
}

fn next_board(
    manager: &Manager,
    carried: &str,
    attempts: &BTreeMap<BoardId, Attempts>,
) -> Option<BoardId> {
    let boards = manager.fleet.lock().ok()?.boards();
    boards
        .iter()
        .filter(|board| board.online && board.beacon.trusts_a_key)
        .filter(|board| is_older(&board.beacon.build, carried))
        .find(|board| attempts.get(&board.id).is_none_or(Attempts::may_try))
        .map(|board| board.id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_higher_number_is_newer() {
        assert!(is_older("b9", "b10"));
        assert!(!is_older("b10", "b9"));
        assert!(!is_older("b10", "b10"));
    }

    #[test]
    fn a_hand_made_build_is_left_alone() {
        assert!(!is_older("mine", "b10"));
        assert!(!is_older("b9", "dev"));
    }

    #[test]
    fn a_failing_board_rests_after_its_tries() {
        let just_now = Instant::now();
        let failed = |count| Attempts {
            count,
            last: just_now,
        };
        assert!(!failed(1).may_try());
        assert!(!failed(TRIES).may_try());
    }
}
