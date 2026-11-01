//! The real boards, through the manager in `olc-board`.
use super::{read_log, BoardRow, FleetPort, LineError, StartError};
use olc_board::{autoupdate, carried, Args, Extra, Manager};
use olc_control::BoardId;
use olc_node::Runner;
use std::sync::{Arc, Mutex};

pub struct RealFleet {
    manager: Arc<Manager>,
}

impl RealFleet {
    pub fn new() -> Result<Self, String> {
        let manager = Manager::new(carried::firmware_path(None), Vec::new())?;
        Ok(Self { manager })
    }
}

impl FleetPort for RealFleet {
    fn attach(&self, runner: Arc<Mutex<Runner>>) {
        self.manager.watch_ports(runner);
        if let Some(build) = carried::build() {
            autoupdate::start(self.manager.clone(), build);
        }
    }

    fn pump(&self, runner: &mut Runner) {
        self.manager.pump(runner);
    }

    fn board_rows(&self) -> Vec<BoardRow> {
        self.manager.board_rows()
    }

    fn new_boards(&self) -> Vec<String> {
        self.manager.new_boards()
    }

    fn job(&self, key: &str) -> Option<String> {
        self.manager.job(key)
    }

    fn run_line(&self, board: BoardId, line: &str) -> Result<(), LineError> {
        self.manager.run_line(board, line)
    }

    fn ask(&self, board: BoardId, line: &str) -> Result<Option<String>, LineError> {
        self.manager.ask(board, line)
    }

    fn start_update(&self, board: BoardId) {
        self.manager.start_update(board);
    }

    fn start_install(&self, path: &str, name: &str) -> bool {
        self.manager.start_install(path, name)
    }

    fn start_extra(
        &self,
        extra: &'static dyn Extra,
        boards: &[BoardId],
        args: Args,
    ) -> Result<(), StartError> {
        self.manager.start_extra(extra, boards, args)
    }

    fn run_log(&self) -> (Vec<String>, bool) {
        read_log(&self.manager.run)
    }

    fn stop_extra(&self) {
        self.manager.stop_extra();
    }
}
