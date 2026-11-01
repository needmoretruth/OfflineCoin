//! Boards told to do something that restarts them. A board on its way back is not a fault.
use olc_board::manager::BoardRow;
use olc_control::BoardId;
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

/// The first words of the lines a board restarts for.
const RESTARTING_LINES: [&str; 6] = ["power", "clock", "full", "network", "wifi", "dial"];
/// A board longer away than this is not coming back by itself.
const WAIT: Duration = Duration::from_secs(120);
/// What the Boards page shows for such a board, the same word an update uses.
const RESTARTING: &str = "restarting";
const FAILED: &str = "update_failed";

struct Expected {
    since: Instant,
    went_quiet: bool,
}

#[derive(Default)]
pub struct Returning {
    boards: BTreeMap<BoardId, Expected>,
}

impl Returning {
    pub fn note_line(&mut self, id: BoardId, line: &str) {
        let word = line.split_whitespace().next().unwrap_or_default();
        if RESTARTING_LINES.contains(&word) {
            let since = Instant::now();
            self.boards.insert(id, Expected { since, went_quiet: false });
        }
    }

    /// Marks the boards on their way back. It ends when one is heard again after going quiet.
    pub fn mark(&mut self, rows: &mut [BoardRow]) {
        for row in rows {
            let id = row.seen.id;
            let Some(expected) = self.boards.get_mut(&id) else {
                continue;
            };
            expected.went_quiet |= !row.seen.online;
            if (expected.went_quiet && row.seen.online) || expected.since.elapsed() > WAIT {
                self.boards.remove(&id);
            } else if !row.seen.online && row.job.is_none() {
                row.job = Some(RESTARTING.into());
            }
        }
    }
}

/// Whether a board that says nothing has a good reason to.
pub fn is_returning(row: &BoardRow) -> bool {
    !row.seen.online && row.job.as_deref().is_some_and(|job| job != FAILED)
}
