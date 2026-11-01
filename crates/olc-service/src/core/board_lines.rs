//! Lines sent to a board, on threads of their own: a board can take seconds to answer.
use super::board_action::BoardAction;
use super::service::LateNotices;
use crate::fleet::{FleetPort, LineError};
use olc_control::BoardId;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::thread;

/// What boards said, kept until they say otherwise.
#[derive(Clone, Default)]
pub struct Told {
    pub bright: Option<u8>,
    /// The last answer to something a person asked.
    pub answer: Option<String>,
}

pub type Brights = Arc<Mutex<BTreeMap<BoardId, Told>>>;

fn note(brights: &Brights, id: BoardId, bright: Option<u8>) {
    if let (Some(bright), Ok(mut brights)) = (bright, brights.lock()) {
        brights.entry(id).or_default().bright = Some(bright);
    }
}

fn keep_answer(brights: &Brights, id: BoardId, answer: Option<String>) {
    if let (Some(answer), Ok(mut brights)) = (answer, brights.lock()) {
        brights.entry(id).or_default().answer = Some(answer);
    }
}

pub fn read_bright(fleet: Arc<dyn FleetPort>, brights: Brights, id: BoardId) {
    thread::spawn(move || note(&brights, id, fleet.bright_of(id)));
}

/// Runs an action and leaves a notice and its answer.
pub fn run_action(
    fleet: Arc<dyn FleetPort>,
    (late, brights): (LateNotices, Brights),
    id: BoardId,
    action: BoardAction,
) {
    let shown = action.shown();
    let Some(line) = action.line() else {
        if let Ok(mut late) = late.lock() {
            late.push(("notice.board_failed", shown, false));
        }
        return;
    };
    thread::spawn(move || {
        let cable = action.needs_cable();
        let asked = if cable { fleet.cable_ask(id, &line) } else { fleet.ask(id, &line) };
        let (key, good) = match asked {
            Ok(answer) => {
                keep_answer(&brights, id, answer);
                let level = line.strip_prefix("bright ").and_then(|word| word.parse().ok());
                note(&brights, id, level);
                ("notice.board_done", true)
            }
            Err(LineError::Refused) => ("notice.board_refused", false),
            Err(LineError::NoAnswer) => ("notice.board_silent", false),
            Err(LineError::BadRequest) if cable => ("notice.board_needs_cable", false),
            Err(LineError::NotDone | LineError::BadRequest) => ("notice.board_failed", false),
        };
        if let Ok(mut late) = late.lock() {
            late.push((key, shown, good));
        }
    });
}
