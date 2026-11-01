//! Keeps every trusted board on the build this program carries.
use crate::control_key::config_folder;
use crate::fleet::Seen;
use crate::manager::Manager;
use olc_control::BoardId;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const HOLD_FILE: &str = "hold-builds";
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

/// The only boards an update may touch. Empty touches none.
pub type Picked = Arc<Mutex<BTreeSet<BoardId>>>;

/// Runs for as long as the program does. `carried` is the build to keep boards on.
pub fn start(manager: Arc<Manager>, carried: &'static str) {
    keep_going(manager, carried, |_| true);
}

/// The same, for the boards a person picked and no others.
pub fn start_for(manager: Arc<Manager>, carried: &'static str, picked: Picked) {
    keep_going(manager, carried, move |board| {
        picked.lock().is_ok_and(|picked| picked.contains(board))
    });
}

fn keep_going(
    manager: Arc<Manager>,
    carried: &'static str,
    allowed: impl Fn(&BoardId) -> bool + Send + 'static,
) {
    thread::spawn(move || {
        let mut attempts: BTreeMap<BoardId, Attempts> = BTreeMap::new();
        loop {
            thread::sleep(LOOK_EVERY);
            if manager.is_busy() || builds_are_held() {
                continue;
            }
            let boards = manager.fleet.lock().map(|fleet| fleet.boards());
            let boards = boards.unwrap_or_default();
            let Some(board) = next_board(&boards, carried, &attempts, &allowed) else {
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

/// Nodes need the same rules, not the same program, so a test build may stay on a board for as
/// long as this file exists.
fn builds_are_held() -> bool {
    config_folder().is_ok_and(|folder| folder.join(HOLD_FILE).exists())
}

fn next_board(
    boards: &[Seen],
    carried: &str,
    attempts: &BTreeMap<BoardId, Attempts>,
    allowed: &impl Fn(&BoardId) -> bool,
) -> Option<BoardId> {
    let waiting = boards
        .iter()
        .filter(|board| allowed(&board.id))
        .filter(|board| board.online && board.beacon.trusts_a_key)
        .filter(|board| is_older(&board.beacon.build, carried))
        .filter(|board| attempts.get(&board.id).is_none_or(Attempts::may_try));
    // Boards with a cable go first, because they are where a build proves itself.
    waiting
        .min_by_key(|board| !matches!(board.connection, Connection::Direct { .. }))
        .map(|board| board.id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use olc_control::Beacon;

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
    fn only_a_picked_board_is_touched() {
        let board = |id: u8| Seen {
            id: [id; 6],
            beacon: Beacon {
                build: "b1".into(),
                trusts_a_key: true,
                ..Beacon::default()
            },
            connection: Connection::Direct { link: 0 },
            online: true,
            silent_for: Duration::ZERO,
        };
        let boards = [board(1), board(2)];
        let attempts = BTreeMap::new();
        assert_eq!(next_board(&boards, "b2", &attempts, &|_| false), None);
        let second = next_board(&boards, "b2", &attempts, &|id| *id == [2; 6]);
        assert_eq!(second, Some([2; 6]));
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
