//! Lines typed into the app's console. The words and what they reach are the terminal's own: a
//! typed line ends in the same [`Command`] as the button beside it.
use super::commands::{Command, Personal};
use super::service::Core;
use super::snapshot::{BoardView, Snapshot};
use super::words::{run_line, usage, Desk};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

/// A board answers within this, or never.
const ANSWER_WAIT: Duration = Duration::from_secs(12);
/// Long enough for the slowest own order, a password's hash on a busy computer.
const ORDER_WAIT: Duration = Duration::from_secs(20);
const LOOK_EVERY: Duration = Duration::from_millis(100);

/// Sends `command` and waits for the notice it causes. No notice in time is an empty answer.
pub(super) fn answer_to(
    snapshot: &dyn Fn() -> Arc<Snapshot>,
    send: &dyn Fn(Command),
    command: Command,
) -> Result<String, String> {
    let board = match command {
        Command::Board { id, .. } => Some(id),
        _ => None,
    };
    let number = snapshot().notice.as_ref().map_or(0, |notice| notice.number);
    send(command);
    let started = Instant::now();
    let wait = if board.is_some() { ANSWER_WAIT } else { ORDER_WAIT };
    while started.elapsed() < wait {
        let seen = snapshot();
        if let Some(notice) = seen.notice.as_ref().filter(|notice| notice.number > number) {
            let said = notice.key.trim_start_matches("notice.").replace('_', " ");
            if !notice.good {
                return Err(format!("{said}: {}", notice.detail));
            }
            let asked = seen.boards.iter().find(|view| Some(view.seen.id) == board);
            let answer = asked.and_then(|view| view.answer.as_deref());
            return Ok(answer.unwrap_or(&notice.detail).to_string());
        }
        thread::sleep(LOOK_EVERY);
    }
    Ok(String::new())
}

struct ConsoleDesk {
    core: Core,
    password: Option<String>,
}

impl Desk for ConsoleDesk {
    fn command(&mut self, command: Command) -> Result<String, String> {
        let looked_up = matches!(command, Command::Own(Personal::Explore(_)));
        let core = &self.core;
        let answer = answer_to(&|| core.snapshot(), &|one| core.send(one), command);
        if looked_up {
            return Ok(format!("{:#?}", core.snapshot().node.explored));
        }
        answer
    }

    fn boards(&mut self) -> Result<Vec<BoardView>, String> {
        Ok(self.core.snapshot().boards.clone())
    }

    fn password(&mut self) -> Result<String, String> {
        self.password.take().ok_or_else(|| "This word needs a password.".to_string())
    }
}

impl Core {
    /// Runs a typed line on its own thread, since a board may take seconds to answer.
    pub fn console(&self, line: String, password: Option<String>) -> Receiver<Result<String, String>> {
        let (answer, receiver) = channel();
        let core = self.clone();
        thread::spawn(move || {
            let result = match line.trim() {
                "help" => Ok(usage()),
                typed => run_line(&mut ConsoleDesk { core, password }, typed),
            };
            let _ = answer.send(result);
        });
        receiver
    }
}
