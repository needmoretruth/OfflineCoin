//! The boards behind one small interface, which is the manager in `olc-board`. The stand-in used
//! by tests is compiled only for tests, so nothing the app ships can show an invented board.
mod real;
#[cfg(test)]
pub mod sim;

pub use olc_board::manager::{BoardRow, ExtraRun, LineError, StartError};
pub use real::RealFleet;

use olc_board::{Args, Extra};
use olc_control::BoardId;
use olc_node::{Hello, Runner};
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
    fn new_boards(&self) -> Vec<String>;
    fn job(&self, key: &str) -> Option<String>;
    /// Blocks for up to a few seconds.
    fn run_line(&self, board: BoardId, line: &str) -> Result<(), LineError>;
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
