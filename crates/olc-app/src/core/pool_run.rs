//! A pool running on this app's node: its workers are real node processes and boards, reached
//! with the same generic work lines anyone could send them.
use super::host::{unix_ms, Host};
use crate::fleet::{FleetPort, LineError};
use crate::local::{NodeSpec, Supervisor};
use olc_control::BoardId;
use olc_core::{Address, BlockHeader};
use olc_node::Engine;
use olc_pool::{Pool, Verdict};
use olc_wallet::parse_address;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const ASK_BOARDS_EVERY: Duration = Duration::from_secs(2);
type Fleet<'a> = Option<&'a Arc<dyn FleetPort>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Worker {
    Node(u32),
    Board(BoardId),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PoolRow {
    pub name: String,
    pub shares: u64,
    pub rejected: u64,
    pub paid: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PoolView {
    pub running: bool,
    pub share_difficulty: u64,
    pub blocks: u64,
    pub lost: u64,
    pub rows: Vec<PoolRow>,
}

pub struct PoolRun {
    pool: Pool,
    workers: BTreeMap<Worker, (Address, String)>,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    (0..text.len() / 2)
        .map(|at| u8::from_str_radix(text.get(at * 2..at * 2 + 2)?, 16).ok())
        .collect()
}

/// Reads what a worker sent back: headers in hex, split by spaces.
pub fn shares_in(text: &str) -> Vec<BlockHeader> {
    text.split_whitespace()
        .filter_map(|word| BlockHeader::decode(&unhex(word)?))
        .collect()
}

/// A line from a board that is not work is not a share. Either way the board stays in the pool.
pub fn shares_of_answer(answer: Result<Option<String>, LineError>) -> Vec<BlockHeader> {
    let answer = answer.ok().flatten().unwrap_or_default();
    shares_in(answer.strip_prefix("shares").unwrap_or(""))
}

impl PoolRun {
    pub fn new(payout: Address, share_difficulty: u64) -> Self {
        Self {
            pool: Pool::new(payout, share_difficulty.max(1)),
            workers: BTreeMap::new(),
        }
    }

    pub fn add_worker(&mut self, worker: Worker, address: Address, name: String) {
        self.workers.insert(worker, (address, name));
    }

    pub fn workers(&self) -> impl Iterator<Item = Worker> + '_ {
        self.workers.keys().copied()
    }

    /// Takes the shares that came in, pays what is ripe, and returns the lines to send.
    pub fn step(
        &mut self,
        engine: &mut Engine,
        shares: Vec<(Worker, BlockHeader)>,
        now: u64,
    ) -> Vec<(Worker, String)> {
        for (worker, header) in shares {
            if let Some((address, _)) = self.workers.get(&worker) {
                // A block changes the tip, and with it every job.
                let _: Verdict = self.pool.submit(engine, *address, &header);
            }
        }
        self.pool.pay(engine);
        if !self.pool.is_out_of_date(engine) {
            return Vec::new();
        }
        let pool = &mut self.pool;
        let jobs = self.workers.iter().map(|(worker, (address, _))| {
            let job = pool.job_for(engine, *address, now);
            let line = format!("work {} {}", hex(&job.header.encode()), job.difficulty);
            (*worker, line)
        });
        jobs.collect()
    }

    pub fn view(&self) -> PoolView {
        let name_of = |address: &Address| {
            let named = self.workers.values().find(|(known, _)| known == address);
            named.map(|(_, name)| name.clone()).unwrap_or_default()
        };
        let rows = self.pool.tally().map(|(address, tally)| PoolRow {
            name: name_of(address),
            shares: tally.shares,
            rejected: tally.rejected,
            paid: tally.paid,
        });
        PoolView {
            running: true,
            share_difficulty: self.pool.share_difficulty(),
            blocks: self.pool.blocks_found(),
            lost: self.pool.blocks_lost(),
            rows: rows.collect(),
        }
    }
}

/// What a pool starts with.
pub struct PoolStart<'a> {
    pub share_difficulty: u64,
    pub specs: &'a [NodeSpec],
    pub nodes: &'a [u32],
    pub boards: &'a [BoardId],
}

/// The pool's place in the app: at most one runs, on the app's own node.
#[derive(Default)]
pub struct PoolSeat {
    run: Option<PoolRun>,
    view: PoolView,
    shares: Vec<(Worker, BlockHeader)>,
    board_shares: Arc<Mutex<Vec<(Worker, BlockHeader)>>>,
    asked: Option<Instant>,
}

fn tell(worker: Worker, line: String, local: &mut Supervisor, fleet: Fleet) {
    match (worker, fleet.cloned()) {
        (Worker::Node(id), _) => drop(local.send(id, &line)),
        (Worker::Board(id), Some(fleet)) => {
            thread::spawn(move || {
                let _ = fleet.run_line(id, &line);
            });
        }
        _ => {}
    }
}

impl PoolSeat {
    pub fn view(&self) -> PoolView {
        self.view.clone()
    }

    pub fn start(&mut self, host: &Host, local: &mut Supervisor, fleet: Fleet, start: &PoolStart) {
        self.stop(local, fleet);
        let mut run = PoolRun::new(host.address(), start.share_difficulty);
        for spec in start.specs.iter().filter(|spec| start.nodes.contains(&spec.id)) {
            let status = local.view(spec).status;
            if let Some(address) = status.and_then(|status| parse_address(&status.address)) {
                run.add_worker(Worker::Node(spec.id), address, spec.name.clone());
            }
        }
        let rows = fleet.map(|fleet| fleet.board_rows()).unwrap_or_default();
        for row in rows.iter().filter(|row| start.boards.contains(&row.seen.id)) {
            let beacon = &row.seen.beacon;
            run.add_worker(Worker::Board(row.seen.id), beacon.address, beacon.name.clone());
        }
        self.run = Some(run);
    }

    pub fn stop(&mut self, local: &mut Supervisor, fleet: Fleet) {
        if let Some(run) = self.run.take() {
            self.view = PoolView::default();
            for worker in run.workers() {
                tell(worker, "work stop".into(), local, fleet);
            }
        }
    }

    pub fn take_node_shares(&mut self, id: u32, shares: &[String]) {
        let found = shares.iter().flat_map(|text| shares_in(text));
        self.shares.extend(found.map(|header| (Worker::Node(id), header)));
    }

    fn ask_boards(&mut self, boards: Vec<Worker>, fleet: Fleet) {
        let due = self.asked.is_none_or(|asked| asked.elapsed() >= ASK_BOARDS_EVERY);
        let (Some(fleet), true) = (fleet.cloned(), due) else {
            return;
        };
        self.asked = Some(Instant::now());
        let found = self.board_shares.clone();
        thread::spawn(move || {
            for worker in boards {
                if let Worker::Board(id) = worker {
                    let shares = shares_of_answer(fleet.ask(id, "work shares"));
                    if let Ok(mut found) = found.lock() {
                        found.extend(shares.into_iter().map(|header| (worker, header)));
                    }
                }
            }
        });
    }

    pub fn step(&mut self, host: &Host, local: &mut Supervisor, fleet: Fleet) {
        let Some(mut run) = self.run.take() else {
            return;
        };
        let boards = run.workers().filter(|w| matches!(w, Worker::Board(_))).collect();
        self.ask_boards(boards, fleet);
        let mut shares = std::mem::take(&mut self.shares);
        if let Ok(mut found) = self.board_shares.lock() {
            shares.append(&mut found);
        }
        let lines = host
            .runner
            .lock()
            .map(|mut runner| run.step(&mut runner.engine, shares, unix_ms()));
        for (worker, line) in lines.unwrap_or_default() {
            tell(worker, line, local, fleet);
        }
        self.view = run.view();
        self.run = Some(run);
    }
}
