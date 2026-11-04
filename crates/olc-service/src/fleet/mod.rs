//! The boards behind one small interface, which is the manager in `olc-board`.
mod cap_test;
mod real;

pub use cap_test::CapTest;
pub use olc_board::{Args, BoardState, Connection, Extra, LentHeard, Seen};
pub use olc_board::manager::{BoardRow, ExtraRun, LineError, StartError};
pub use real::RealFleet;

use olc_control::BoardId;
use olc_node::{Hello, Runner};
use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

/// The same shape as `olc_board::Manager`.
pub trait FleetPort: Send + Sync {
    /// Starts looking for boards, on the runner's links.
    fn attach(&self, runner: Arc<Mutex<Runner>>);
    /// Called from the loop that steps the runner.
    fn pump(&self, runner: &mut Runner);
    fn board_rows(&self) -> Vec<BoardRow>;
    /// What a board said about itself over the node layer, for boards that can.
    fn hello_of(&self, _board: BoardId) -> Option<Hello> {
        None
    }
    /// The panel's stored brightness, as the board says. Blocks for up to a few seconds.
    fn state_of(&self, _board: BoardId) -> Option<BoardState> {
        None
    }
    /// The only boards an automatic update may touch.
    fn keep_updated(&self, _boards: BTreeSet<BoardId>) {}
    fn new_boards(&self) -> Vec<String>;
    fn job(&self, key: &str) -> Option<String>;
    /// Blocks for up to a few seconds.
    fn run_line(&self, board: BoardId, line: &str) -> Result<(), LineError>;
    /// Asks down the board's own cable. A board with no cable here refuses it.
    fn cable_ask(&self, _board: BoardId, _line: &str) -> Result<Option<String>, LineError> {
        Err(LineError::BadRequest)
    }
    /// Like `run_line`, with what the board answered.
    fn ask(&self, board: BoardId, line: &str) -> Result<Option<String>, LineError>;
    /// Signs pool work for the board that mines to `worker` and sends it through `relay`, the
    /// board that hears it. `None` stops the work. Says whether it went out.
    fn lend(&self, _relay: BoardId, _worker: [u8; 20], _work: Option<(Vec<u8>, u64)>) -> bool {
        false
    }
    /// What the board that mines to `worker` found for lent work since the last call.
    fn take_lent_shares(&self, _worker: &[u8; 20]) -> Vec<Vec<u8>> {
        Vec::new()
    }
    /// When lent work for `worker` was last passed on by the board on the cable, and taken.
    fn lent_heard(&self, _worker: &[u8; 20]) -> LentHeard {
        LentHeard::default()
    }
    fn start_update(&self, board: BoardId);
    fn start_install(&self, path: &str, name: &str) -> bool;
    fn start_extra(
        &self,
        extra: &'static dyn Extra,
        boards: &[BoardId],
        args: Args,
    ) -> Result<(), StartError>;
    /// The lines of the last experiment, and whether it still runs.
    fn run_log(&self) -> (Vec<String>, bool);
    fn stop_extra(&self);
}

pub fn read_log(run: &Mutex<ExtraRun>) -> (Vec<String>, bool) {
    run.lock()
        .map(|run| (run.lines.iter().cloned().collect(), run.running))
        .unwrap_or_default()
}
