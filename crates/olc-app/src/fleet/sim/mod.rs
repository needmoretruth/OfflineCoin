//! A fleet with no hardware. Each board is a real node engine with a simulated hash rate, joined by
//! a simulated radio, and answers the same command lines a real board does. So the app and the
//! experiments run for real, and only the silicon is missing.
mod board;
mod radio;

use super::{read_log, BoardRow, ExtraRun, FleetPort, LineError, StartError};
use board::{SimBoard, SimLink};
use olc_board::{Args, BoardLink, Connection, Extra, Say, Seen};
use olc_control::{BoardId, Beacon};
use olc_core::CoinConfig;
use olc_node::{Hello, Runner};
use radio::Radio;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

const STEP: Duration = Duration::from_millis(50);
const MAX_LOG: usize = 400;
const UPDATE_STEPS: u32 = 10;

/// What the fleet starts with.
struct Start {
    name: &'static str,
    on_cable: bool,
    through: Option<usize>,
    coin: usize,
    balance_first: bool,
}

const FLEET: [Start; 4] = [
    Start { name: "desk-a", on_cable: true, through: None, coin: 0, balance_first: true },
    Start { name: "desk-b", on_cable: true, through: None, coin: 0, balance_first: false },
    Start { name: "shelf", on_cable: false, through: Some(0), coin: 0, balance_first: false },
    Start { name: "garage", on_cable: false, through: Some(1), coin: 1, balance_first: false },
];

/// The coins the simulated boards run: one network and one that split from it by rule.
pub fn sim_coins() -> [CoinConfig; 2] {
    let mut bench = CoinConfig::olc();
    bench.name = "Bench".into();
    bench.ticker = "BNC".into();
    bench.message = "Bench genesis".into();
    bench.initial_difficulty = 3_000;
    bench.rules.block_time_ms = 2_000;
    bench.rules.coinbase_maturity = 1;
    let mut split = bench.clone();
    split.rules.block_time_ms = 1_000;
    [bench, split]
}

pub struct SimFleet {
    boards: Arc<Mutex<Vec<SimBoard>>>,
    run: Arc<Mutex<ExtraRun>>,
    jobs: Arc<Mutex<BTreeMap<String, String>>>,
    ports: Mutex<Vec<String>>,
}

impl Default for SimFleet {
    fn default() -> Self {
        Self::new()
    }
}

impl SimFleet {
    pub fn new() -> Self {
        let coins = sim_coins();
        let radios = [Radio::default(), Radio::default()];
        let boards: Vec<SimBoard> = FLEET
            .iter()
            .enumerate()
            .map(|(index, start)| {
                SimBoard::new(
                    index as u8,
                    start.name,
                    coins[start.coin].clone(),
                    radios[start.coin].join(),
                    start.balance_first,
                )
            })
            .collect();
        let boards = Arc::new(Mutex::new(boards));
        let stepping = boards.clone();
        thread::spawn(move || loop {
            if let Ok(mut boards) = stepping.lock() {
                boards.iter_mut().for_each(|board| board.step(STEP));
            }
            thread::sleep(STEP);
        });
        Self {
            boards,
            run: Arc::default(),
            jobs: Arc::default(),
            ports: Mutex::new(vec!["/dev/serial/by-id/usb-Espressif_new-board".to_string()]),
        }
    }

    fn with_board<T>(&self, id: BoardId, act: impl FnOnce(&mut SimBoard) -> T) -> Option<T> {
        let mut boards = self.boards.lock().ok()?;
        boards.iter_mut().find(|board| board.id() == id).map(act)
    }
}

fn row_of(index: usize, board: &SimBoard, names: &[(BoardId, String)]) -> BoardRow {
    let start = FLEET.get(index);
    let on_cable = start.is_none_or(|start| start.on_cable);
    let through = start.and_then(|start| start.through);
    let beacon: Beacon = board.beacon();
    BoardRow {
        seen: Seen {
            id: board.id(),
            beacon,
            connection: match through {
                Some(at) => Connection::Through {
                    board: names.get(at).map(|(id, _)| *id),
                },
                None => Connection::Direct { link: 1 + index },
            },
            online: true,
            silent_for: Duration::ZERO,
        },
        on_cable,
        through: through.and_then(|at| names.get(at).map(|(_, name)| name.clone())),
        job: None,
    }
}

impl FleetPort for SimFleet {
    fn attach(&self, _runner: Arc<Mutex<Runner>>) {}

    fn pump(&self, _runner: &mut Runner) {}

    fn board_rows(&self) -> Vec<BoardRow> {
        let Ok(boards) = self.boards.lock() else {
            return Vec::new();
        };
        let names: Vec<(BoardId, String)> = boards
            .iter()
            .map(|board| (board.id(), board.name().to_string()))
            .collect();
        let jobs = self.jobs.lock().map(|jobs| jobs.clone()).unwrap_or_default();
        boards
            .iter()
            .enumerate()
            .map(|(index, board)| {
                let mut row = row_of(index, board, &names);
                row.job = jobs.get(&board::hex(&board.id())).cloned();
                row
            })
            .collect()
    }

    fn hello_of(&self, board: BoardId) -> Option<Hello> {
        self.with_board(board, |board| board.hello())
    }

    fn new_boards(&self) -> Vec<String> {
        self.ports.lock().map(|ports| ports.clone()).unwrap_or_default()
    }

    fn job(&self, key: &str) -> Option<String> {
        self.jobs.lock().ok()?.get(key).cloned()
    }

    fn run_line(&self, board: BoardId, line: &str) -> Result<(), LineError> {
        self.ask(board, line).map(|_| ())
    }

    fn ask(&self, board: BoardId, line: &str) -> Result<Option<String>, LineError> {
        if !olc_control::allowed_remotely(line) {
            return Err(LineError::BadRequest);
        }
        match self.with_board(board, |board| board.command(line)) {
            Some(answer) if answer.starts_with("error") => Err(LineError::NotDone),
            Some(answer) => Ok(Some(answer)),
            None => Err(LineError::NoAnswer),
        }
    }

    fn start_update(&self, board: BoardId) {
        let key = board::hex(&board);
        let (jobs, boards) = (self.jobs.clone(), self.boards.clone());
        thread::spawn(move || {
            for step in 1..=UPDATE_STEPS {
                let text = format!("{}%", step * 100 / UPDATE_STEPS);
                if let Ok(mut jobs) = jobs.lock() {
                    jobs.insert(key.clone(), text);
                }
                thread::sleep(Duration::from_millis(250));
            }
            if let Ok(mut jobs) = jobs.lock() {
                jobs.remove(&key);
            }
            if let Ok(mut boards) = boards.lock() {
                boards
                    .iter_mut()
                    .filter(|found| found.id() == board)
                    .for_each(SimBoard::mark_updated);
            }
        });
    }

    fn start_install(&self, path: &str, name: &str) -> bool {
        let Ok(mut ports) = self.ports.lock() else {
            return false;
        };
        let Some(at) = ports.iter().position(|port| port == path) else {
            return false;
        };
        ports.remove(at);
        if let Ok(mut boards) = self.boards.lock() {
            let index = boards.len() as u8;
            let coin = sim_coins()[0].clone();
            boards.push(SimBoard::new(index, name, coin, Radio::default().join(), false));
        }
        true
    }

    fn start_extra(
        &self,
        extra: &'static dyn Extra,
        boards: &[BoardId],
        args: Args,
    ) -> Result<(), StartError> {
        let stop = Arc::new(AtomicBool::new(false));
        {
            let mut run = self.run.lock().map_err(|_| StartError::Busy)?;
            if run.running {
                return Err(StartError::Busy);
            }
            run.lines.clear();
            run.running = true;
            run.stop = Some(stop.clone());
        }
        let links: Vec<Box<dyn BoardLink>> = boards
            .iter()
            .map(|id| Box::new(SimLink::new(self.boards.clone(), *id)) as Box<dyn BoardLink>)
            .collect();
        let run = self.run.clone();
        let lines = run.clone();
        let say: Say = Arc::new(move |line| push_line(&lines, line));
        thread::spawn(move || {
            if let Err(error) = extra.run_on(links, &args, say, stop) {
                push_line(&run, error);
            }
            if let Ok(mut run) = run.lock() {
                run.running = false;
                run.stop = None;
            }
        });
        Ok(())
    }

    fn run_log(&self) -> (Vec<String>, bool) {
        read_log(&self.run)
    }

    fn stop_extra(&self) {
        if let Ok(run) = self.run.lock() {
            if let Some(stop) = &run.stop {
                stop.store(true, Ordering::Relaxed);
            }
        }
    }
}

fn push_line(run: &Mutex<ExtraRun>, line: String) {
    if let Ok(mut run) = run.lock() {
        if run.lines.len() >= MAX_LOG {
            run.lines.pop_front();
        }
        run.lines.push_back(line);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fleet() -> SimFleet {
        SimFleet::new()
    }

    #[test]
    fn the_simulated_fleet_shows_every_way_a_board_is_reached() {
        let rows = fleet().board_rows();
        assert_eq!(rows.len(), FLEET.len());
        assert!(rows.iter().filter(|row| row.on_cable).count() >= 2);
        let shelf = rows.iter().find(|row| row.seen.beacon.name == "shelf").unwrap();
        assert_eq!(shelf.through.as_deref(), Some("desk-a"));
    }

    #[test]
    fn a_cap_sent_as_a_line_lands_on_the_board() {
        let fleet = fleet();
        let id = fleet.board_rows()[0].seen.id;
        fleet.run_line(id, "limit 1234").unwrap();
        let row = fleet.board_rows().into_iter().next().unwrap();
        assert_eq!(row.seen.beacon.limit, 1234);
    }

    #[test]
    fn a_line_only_the_cable_may_send_is_refused() {
        let fleet = fleet();
        let id = fleet.board_rows()[0].seen.id;
        assert_eq!(fleet.run_line(id, "erase"), Err(LineError::BadRequest));
    }

    #[test]
    fn a_new_port_becomes_a_board_when_installed() {
        let fleet = fleet();
        let port = fleet.new_boards().remove(0);
        assert!(fleet.start_install(&port, "new one"));
        assert!(fleet.new_boards().is_empty());
        assert_eq!(fleet.board_rows().len(), FLEET.len() + 1);
    }

    #[test]
    fn boards_of_one_coin_mine_and_hear_each_other() {
        let fleet = fleet();
        let id = |name: &str| {
            let rows = fleet.board_rows();
            rows.iter().find(|row| row.seen.beacon.name == name).unwrap().seen.id
        };
        fleet.run_line(id("desk-a"), "mine on").unwrap();
        let top = |name: &str| fleet.hello_of(id(name)).unwrap().height;
        let mut waited = 0;
        while (top("desk-a") == 0 || top("shelf") == 0) && waited < 300 {
            thread::sleep(Duration::from_millis(100));
            waited += 1;
        }
        assert!(top("desk-a") > 0);
        assert!(top("shelf") > 0);
    }
}
