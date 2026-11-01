//! Lines sent to a board, on threads of their own: a board can take seconds to answer.
use super::service::LateNotices;
use crate::fleet::{FleetPort, LineError};
use olc_control::BoardId;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::thread;

/// What boards said about their panels, kept until they say otherwise.
pub type Brights = Arc<Mutex<BTreeMap<BoardId, u8>>>;

fn note(brights: &Brights, id: BoardId, bright: Option<u8>) {
    if let (Some(bright), Ok(mut brights)) = (bright, brights.lock()) {
        brights.insert(id, bright);
    }
}

pub fn read_bright(fleet: Arc<dyn FleetPort>, brights: Brights, id: BoardId) {
    thread::spawn(move || note(&brights, id, fleet.bright_of(id)));
}

pub fn run_line(
    fleet: Arc<dyn FleetPort>,
    (late, brights): (LateNotices, Brights),
    id: BoardId,
    line: String,
) {
    thread::spawn(move || {
        let (key, good) = match fleet.run_line(id, &line) {
            Ok(()) => {
                let level = line.strip_prefix("bright ").and_then(|word| word.parse().ok());
                note(&brights, id, level);
                ("notice.board_done", true)
            }
            Err(LineError::Refused) => ("notice.board_refused", false),
            Err(LineError::NoAnswer) => ("notice.board_silent", false),
            Err(LineError::NotDone | LineError::BadRequest) => ("notice.board_failed", false),
        };
        if let Ok(mut late) = late.lock() {
            late.push((key, line, good));
        }
    });
}
