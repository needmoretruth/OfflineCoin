//! Lines typed down a board's own cable. The orders a radio may never carry go this way, so
//! whoever gives them has the board plugged into this computer.
use crate::manager::{LineError, Manager};
use crate::Connection;
use olc_control::BoardId;
use std::thread;
use std::time::{Duration, Instant};

/// A board that says nothing for this long has finished what it had to say, or restarted.
const ANSWER_WAIT: Duration = Duration::from_secs(2);
const QUIET_AFTER: Duration = Duration::from_millis(300);
const LOOK_EVERY: Duration = Duration::from_millis(50);

impl Manager {
    /// Sends `line` down the cable of `board` and returns what came back.
    pub fn cable_ask(&self, board: BoardId, line: &str) -> Result<Option<String>, LineError> {
        let row = self.board_rows().into_iter().find(|row| row.seen.id == board);
        let link = match row.map(|row| row.seen.connection) {
            Some(Connection::Direct { link }) => link,
            _ => return Err(LineError::BadRequest),
        };
        let cable = self.cable_of_link(link).ok_or(LineError::BadRequest)?;
        if let Ok(mut heard) = cable.heard.lock() {
            heard.clear();
        }
        if !cable.typed.send(line) {
            return Err(LineError::NoAnswer);
        }
        let started = Instant::now();
        let mut answer = Vec::new();
        let mut last = started;
        loop {
            if let Ok(mut heard) = cable.heard.lock() {
                if !heard.is_empty() {
                    answer.extend(heard.drain(..));
                    last = Instant::now();
                }
            }
            let quiet = !answer.is_empty() && last.elapsed() >= QUIET_AFTER;
            if quiet || started.elapsed() >= ANSWER_WAIT {
                break;
            }
            thread::sleep(LOOK_EVERY);
        }
        if answer.first().is_some_and(|first| first.starts_with("error")) {
            return Err(LineError::NotDone);
        }
        Ok((!answer.is_empty()).then(|| answer.join("\n")))
    }
}
