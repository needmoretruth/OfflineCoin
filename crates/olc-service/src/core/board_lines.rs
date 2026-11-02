//! Lines sent to a board, on threads of their own: a board can take seconds to answer.
use super::board_action::BoardAction;
use super::service::LateNotices;
use crate::fleet::{BoardState, FleetPort, LineError};
use olc_control::BoardId;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::thread;

/// What boards said, kept until they say otherwise.
#[derive(Clone, Default)]
pub struct Told {
    pub state: BoardState,
    /// The last answer to something a person asked.
    pub answer: Option<String>,
}

pub type BoardsTold = Arc<Mutex<BTreeMap<BoardId, Told>>>;

fn note(told: &BoardsTold, id: BoardId, state: Option<BoardState>) {
    if let (Some(state), Ok(mut told)) = (state, told.lock()) {
        told.entry(id).or_default().state = state;
    }
}

/// A line the board took changes what it said, until it next says.
fn note_line(told: &BoardsTold, id: BoardId, line: &str) {
    if let Ok(mut told) = told.lock() {
        told.entry(id).or_default().state.after(line);
    }
}

fn keep_answer(told: &BoardsTold, id: BoardId, answer: Option<String>) {
    if let (Some(answer), Ok(mut told)) = (answer, told.lock()) {
        told.entry(id).or_default().answer = Some(answer);
    }
}

pub fn read_state(fleet: Arc<dyn FleetPort>, told: BoardsTold, id: BoardId) {
    thread::spawn(move || note(&told, id, fleet.state_of(id)));
}

/// Runs an action and leaves a notice and its answer.
pub fn run_action(
    fleet: Arc<dyn FleetPort>,
    (late, told): (LateNotices, BoardsTold),
    id: BoardId,
    action: BoardAction,
) {
    let shown = action.shown();
    let Some(line) = action.line() else {
        let key = action.fault().unwrap_or("notice.board_failed");
        if let Ok(mut late) = late.lock() {
            late.push((key, shown, false));
        }
        return;
    };
    thread::spawn(move || {
        let cable = action.needs_cable();
        let asked = if cable { fleet.cable_ask(id, &line) } else { fleet.ask(id, &line) };
        let (key, good) = match asked {
            Ok(answer) => {
                keep_answer(&told, id, answer);
                note_line(&told, id, &line);
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
