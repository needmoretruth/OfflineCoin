//! The pool this app runs, and the miners here that work in a pool, this app's or another's.
//!
//! Every miner here joins a pool the way a miner anywhere does, in Stratum v1 over TCP: the app's
//! own miner, the miners on this computer and the boards, the pool here included, which they
//! reach on this computer's loopback. Each link hands its miner a header in the generic work line
//! and sends back the shares the miner found.
use super::host::Host;
use super::pool_lend::{lend, lent_name, relay_name, take_lent, Said, Watch};
use super::pool_plan::{pool_address, worker_part, Join, PoolOrder, PoolSettings, Who};
use super::pool_view::{choices, link_trouble, reach, short_address, FoundRow, LinkRow, PoolView, SwitchedOff, WorkerRow};
use crate::fleet::{BoardRow, FleetPort};
use crate::local::{NodeSpec, Supervisor};
use olc_core::BlockHeader;
use olc_node::Hello;
use olc_pool::{unix_ms, worker_address, Client, Pool, Server, Shared};
use olc_wallet::{format_address, parse_address};
use std::collections::BTreeMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

/// A miner gets a fresh header this often, so its block's time moves and it never runs dry.
const REFRESH_WORK: Duration = Duration::from_secs(20);
const ASK_BOARDS_EVERY: Duration = Duration::from_secs(2);
const PAY_EVERY: Duration = Duration::from_secs(5);
const VIEW_EVERY: Duration = Duration::from_millis(500);
/// How long a turn waits for the pool here to answer the shares just sent.
const ANSWER_WITHIN: Duration = Duration::from_millis(50);
/// How long a turn waits for the pool here to hand out the job of a new block.
const NEW_JOB_WITHIN: Duration = Duration::from_millis(150);
/// The name the app's own miner signs in with, after its address.
const OWN_WORKER: &str = "core";

type Fleet<'a> = Option<&'a Arc<dyn FleetPort>>;
type Trouble = Arc<Mutex<Option<&'static str>>>;

/// What the pool seat needs from the rest of the app for one turn.
pub struct PoolCx<'a> {
    pub host: &'a Host,
    pub local: &'a mut Supervisor,
    pub fleet: Fleet<'a>,
    pub specs: &'a [NodeSpec],
    /// Boards heard as nodes, with the link of this app each was heard on.
    pub hellos: &'a [(&'a Hello, Option<usize>)],
    pub settings: &'a mut PoolSettings,
}

struct Running {
    pool: Shared,
    _server: Server,
    port: u16,
}

struct WorkLink {
    join: Join,
    client: Client,
    here: bool,
    sent: Option<(u64, Instant)>,
    extranonce2: u64,
    asking: Arc<AtomicBool>,
    asked: Option<Instant>,
    found: Arc<Mutex<Vec<BlockHeader>>>,
    trouble: Trouble,
    /// For a board heard through another: lent work until the board says it took it.
    watch: Option<Watch>,
}

#[derive(Default)]
pub struct PoolSeat {
    running: Option<Running>,
    links: BTreeMap<Who, WorkLink>,
    node_shares: Vec<(u32, BlockHeader)>,
    resumed: bool,
    paid_at: Option<Instant>,
    viewed: Option<Instant>,
    error: Option<(&'static str, String)>,
    view: PoolView,
    /// The chain's tip at the pool's last turn.
    tip: Option<olc_core::Hash>,
    switched_off: Vec<SwitchedOff>,
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

/// The line any miner here takes work in: a header and the difficulty a share must reach.
fn work_line(header: &BlockHeader, difficulty: u64) -> String {
    format!("work {} {}", hex(&header.encode()), difficulty)
}

fn board_row(fleet: Fleet, id: olc_control::BoardId) -> Option<BoardRow> {
    fleet?.board_rows().into_iter().find(|row| row.seen.id == id)
}

/// The name a miner here signs in with: its own address, so its part is paid to it, and its name.
fn user_for(who: Who, cx: &mut PoolCx) -> Option<String> {
    match who {
        // Paid where its own blocks would go: the address chosen for mining, else the wallet's.
        Who::Own => {
            let miner = cx.host.ready().map(|runner| runner.engine.miner()).ok()?;
            Some(format!("{}.{OWN_WORKER}", format_address(&miner)))
        }
        Who::Node(id) => {
            let spec = cx.specs.iter().find(|spec| spec.id == id)?;
            let status = cx.local.view(spec).status?;
            let address = parse_address(&status.address)?;
            Some(format!("{}.{}", format_address(&address), worker_part(&spec.name)))
        }
        Who::Board(id) => {
            let row = board_row(cx.fleet, id)?;
            let name = worker_part(&row.seen.beacon.name);
            Some(format!("{}.{name}", format_address(&row.seen.beacon.address)))
        }
        Who::Lent(worker) => {
            cx.hellos.iter().find(|(hello, _)| hello.address == worker)?;
            Some(format!("{}.{}", format_address(&worker), worker_part(&lent_name(&worker, cx.hellos))))
        }
    }
}

/// Tells a miner here to stop the pool's work.
fn stop_work(who: Who, cx: &mut PoolCx) {
    match (who, cx.fleet.cloned()) {
        (Who::Own, _) => cx.host.take_work(None),
        (Who::Node(id), _) => drop(cx.local.send(id, "work stop")),
        (Who::Board(id), Some(fleet)) => {
            thread::spawn(move || {
                let _ = fleet.run_line(id, "work stop");
            });
        }
        (Who::Board(_), None) => {}
        (Who::Lent(worker), _) => drop(lend(worker, None, cx)),
    }
}

impl PoolSeat {
    pub fn view(&self) -> PoolView {
        self.view.clone()
    }

    /// Why the last order failed, in the words the system gave.
    pub fn failure(&self) -> String {
        self.error.as_ref().map(|(_, detail)| detail.clone()).unwrap_or_default()
    }

    /// Carries out what a person asked. False when it could not, with the reason in the view.
    pub fn order(&mut self, order: PoolOrder, cx: &mut PoolCx) -> bool {
        self.error = None;
        let done = match order {
            PoolOrder::Start(settings) => self.start(settings, cx),
            PoolOrder::Stop => {
                self.stop(cx);
                cx.settings.run_on_start = false;
                true
            }
            PoolOrder::Join(join) => self.join_elsewhere(join, cx),
            PoolOrder::Leave(who) => {
                cx.settings.joins.retain(|join| join.who != who);
                self.leave(who, cx);
                true
            }
            PoolOrder::Show => true,
        };
        self.viewed = None;
        self.refresh_view(cx);
        done
    }

    fn start(&mut self, settings: PoolSettings, cx: &mut PoolCx) -> bool {
        self.stop(cx);
        let everywhere = settings.open && settings.stand_in.is_none();
        let host = if everywhere { IpAddr::V4(Ipv4Addr::UNSPECIFIED) } else { IpAddr::V4(Ipv4Addr::LOCALHOST) };
        let pool: Shared = Arc::new(Mutex::new(Pool::new(cx.host.address(), settings.plan())));
        if let (Ok(runner), Ok(mut pool)) = (cx.host.ready(), pool.lock()) {
            pool.refresh(&runner.engine, unix_ms());
        }
        let server = match Server::start(SocketAddr::new(host, settings.port), pool.clone()) {
            Ok(server) => server,
            Err(error) => {
                self.error = Some(("pool.error_port", error.to_string()));
                return false;
            }
        };
        // The port the system gave, which is the one asked for unless that was 0.
        let port = server.address().port();
        self.running = Some(Running { pool, _server: server, port });
        let at = format!("127.0.0.1:{port}");
        for who in settings.workers.clone() {
            if let Some(user) = user_for(who, cx) {
                self.link(Join { who, address: at.clone(), user }, true, cx);
            }
        }
        let joins = std::mem::take(&mut cx.settings.joins);
        *cx.settings = PoolSettings { joins, run_on_start: true, ..settings };
        true
    }

    /// Stops the pool here. The miners that worked in it go back to the pools they had joined.
    fn stop(&mut self, cx: &mut PoolCx) {
        if self.running.take().is_none() {
            return;
        }
        let here: Vec<Who> = self.links.iter().filter(|(_, link)| link.here).map(|(who, _)| *who).collect();
        for who in here {
            self.leave(who, cx);
            if let Some(join) = cx.settings.joins.iter().find(|join| join.who == who).cloned() {
                self.link(join, false, cx);
            }
        }
    }

    fn join_elsewhere(&mut self, join: Join, cx: &mut PoolCx) -> bool {
        let Some(address) = pool_address(&join.address) else {
            self.error = Some(("pool.error_address", join.address));
            return false;
        };
        if let Err(said) = worker_address(join.user.trim()) {
            self.error = Some(("pool.error_user", said.to_string()));
            return false;
        }
        let join = Join { who: join.who, address, user: join.user.trim().to_string() };
        cx.settings.joins.retain(|known| known.who != join.who);
        cx.settings.joins.push(join.clone());
        self.leave(join.who, cx);
        self.link(join, false, cx);
        true
    }

    fn link(&mut self, join: Join, here: bool, cx: &mut PoolCx) {
        self.leave(join.who, cx);
        // Choosing this computer for a pool is asking it to mine.
        if join.who == Who::Own {
            cx.host.set_mining(true);
        }
        let client = Client::start(&join.address, &join.user, "x");
        let link = WorkLink {
            join,
            client,
            here,
            sent: None,
            extranonce2: 0,
            asking: Arc::new(AtomicBool::new(false)),
            asked: None,
            found: Arc::new(Mutex::new(Vec::new())),
            trouble: Arc::new(Mutex::new(None)),
            watch: None,
        };
        self.switched_off.retain(|row| row.who != link.join.who);
        self.links.insert(link.join.who, link);
    }

    fn leave(&mut self, who: Who, cx: &mut PoolCx) {
        if self.links.remove(&who).is_some() {
            stop_work(who, cx);
        }
    }

    pub fn take_node_shares(&mut self, id: u32, shares: &[String]) {
        let found = shares.iter().flat_map(|text| shares_in(text));
        self.node_shares.extend(found.map(|header| (id, header)));
    }

    /// One turn: the pool takes in the chain and adds what it found; each miner gets fresh work
    /// and sends what it found. True when it switched a board off, which changes the settings.
    pub fn step(&mut self, cx: &mut PoolCx) -> bool {
        if !self.resumed {
            self.resumed = true;
            self.resume(cx);
        }
        // What the miners found goes before the pool takes in a new block, which ends the jobs it
        // was found on: sent after, every share of the last turn would be refused as old work.
        let node_shares = std::mem::take(&mut self.node_shares);
        for (who, link) in &mut self.links {
            let found: Vec<BlockHeader> = match who {
                Who::Own => cx.host.take_shares(),
                Who::Node(id) => node_shares.iter().filter(|(from, _)| from == id).map(|(_, header)| *header).collect(),
                Who::Board(id) => link.ask_board(*id, cx.fleet),
                Who::Lent(worker) => take_lent(worker, cx),
            };
            for header in &found {
                link.client.submit(header);
            }
            // The pool here answers within a moment; one elsewhere keeps its own order.
            if link.here && !found.is_empty() {
                link.client.settle(None, ANSWER_WITHIN);
            }
        }
        let before: Vec<(Who, u64)> = self.links.iter().filter(|(_, link)| link.here).map(|(who, link)| (*who, link.client.generation())).collect();
        if self.step_pool(cx.host) {
            // A new block ended every job: the miners here wait for the new one rather than grind
            // a turn more on work the pool no longer takes.
            for (who, generation) in before {
                if let Some(link) = self.links.get(&who) {
                    link.client.settle(Some(generation), NEW_JOB_WITHIN);
                }
            }
        }
        let mining = !self.links.contains_key(&Who::Own) || cx.host.view().mining;
        for (who, link) in &mut self.links {
            link.hand_work(*who, mining, cx);
        }
        let silent = self.silent_boards(cx);
        for (who, why) in silent.iter().copied() {
            self.switch_off(who, why, cx);
        }
        self.refresh_view(cx);
        !silent.is_empty()
    }

    /// Boards heard through another that never said they took the work, with why. Work nobody
    /// answered yet goes out again at the next turn.
    fn silent_boards(&mut self, cx: &PoolCx) -> Vec<(Who, &'static str)> {
        let mut silent = Vec::new();
        for (who, link) in &mut self.links {
            let (Who::Lent(worker), Some(watch)) = (who, link.watch.as_mut()) else {
                continue;
            };
            match watch.check(worker, cx) {
                Said::Again => link.sent = None,
                Said::Silent(why) => silent.push((*who, why)),
                Said::Taken | Said::Waiting => {}
            }
        }
        silent
    }

    /// Turns off a board that never took the pool's work, as if its switch were turned off, and
    /// keeps why for its row until it is switched on again.
    fn switch_off(&mut self, who: Who, why: &'static str, cx: &mut PoolCx) {
        let Who::Lent(worker) = who else {
            return;
        };
        let relay = relay_name(&worker, cx);
        let user = self.links.get(&who).map(|link| link.join.user.clone()).unwrap_or_default();
        self.leave(who, cx);
        cx.settings.workers.retain(|known| *known != who);
        cx.settings.joins.retain(|join| join.who != who);
        self.switched_off.retain(|row| row.who != who);
        self.switched_off.push(SwitchedOff { who, user, why, relay, at_ms: unix_ms() });
        self.viewed = None;
    }

    /// After a start: the pool here again if it ran, and the pools the miners here had joined.
    fn resume(&mut self, cx: &mut PoolCx) {
        if cx.settings.run_on_start {
            let settings = cx.settings.clone();
            self.start(settings, cx);
        }
        for join in cx.settings.joins.clone() {
            if !self.links.contains_key(&join.who) {
                self.link(join, false, cx);
            }
        }
    }

    /// The pool's turn with the chain. True when the chain moved, so every job before is over.
    fn step_pool(&mut self, host: &Host) -> bool {
        let Some(running) = &self.running else {
            return false;
        };
        let Ok(mut pool) = running.pool.lock() else {
            return false;
        };
        let Ok(mut runner) = host.ready() else {
            return false;
        };
        let now = unix_ms();
        for block in pool.take_blocks() {
            let id = block.id();
            let added = runner.engine.submit_block(block);
            pool.block_added(id, added);
        }
        pool.refresh(&runner.engine, now);
        if self.paid_at.is_none_or(|paid| paid.elapsed() >= PAY_EVERY) {
            self.paid_at = Some(Instant::now());
            pool.pay(&mut runner.engine);
        }
        let tip = runner.engine.chain().tip();
        let moved = self.tip.is_some_and(|before| before != tip);
        self.tip = Some(tip);
        moved
    }

    fn refresh_view(&mut self, cx: &mut PoolCx) {
        if self.viewed.is_some_and(|viewed| viewed.elapsed() < VIEW_EVERY) {
            return;
        }
        self.viewed = Some(Instant::now());
        let settings = cx.settings.clone();
        let now = unix_ms();
        let mut view = PoolView {
            running: self.running.is_some(),
            port: self.running.as_ref().map_or(settings.port, |running| running.port),
            open: settings.open,
            reach: reach(self.running.as_ref().map_or(settings.port, |running| running.port), settings.open, settings.stand_in),
            method: settings.method,
            last_shares: settings.last_shares,
            fee_bps: settings.fee_bps,
            fixed_difficulty: settings.fixed_difficulty,
            error: self.error.clone(),
            switched_off: self.switched_off.clone(),
            ..PoolView::default()
        };
        let rows = cx.fleet.map(|fleet| fleet.board_rows()).unwrap_or_default();
        let locals: Vec<(u32, String, Option<u64>)> = cx
            .specs
            .iter()
            .map(|spec| {
                let speed = cx.local.view(spec).status.map(|status| status.hashrate);
                (spec.id, spec.name.clone(), speed)
            })
            .collect();
        let node = cx.host.view();
        view.block_difficulty = node.difficulty;
        view.choices = choices(&locals, &rows, cx.hellos, node.hashrate);
        let label = |who: Who| match who {
            Who::Own => String::new(),
            Who::Node(id) => locals.iter().find(|(known, ..)| *known == id).map(|(_, name, _)| name.clone()).unwrap_or_default(),
            Who::Board(id) => rows.iter().find(|row| row.seen.id == id).map(|row| row.seen.beacon.name.clone()).unwrap_or_default(),
            Who::Lent(worker) => lent_name(&worker, cx.hellos),
        };
        view.links = self
            .links
            .iter()
            .map(|(who, link)| {
                let client = link.client.view();
                let mut trouble = link_trouble(&client).or_else(|| link.trouble.lock().ok().and_then(|said| *said));
                if *who == Who::Own && !node.mining {
                    trouble = Some("pool.own_off");
                }
                LinkRow { join: link.join.clone(), label: label(*who), view: client, trouble, here: link.here }
            })
            .collect();
        if let Some(pool) = self.running.as_ref().and_then(|running| running.pool.lock().ok()) {
            let linked = |name: &str| view.links.iter().find(|row| row.here && row.join.user == name).map(|row| row.join.who);
            let off = |name: &str| view.switched_off.iter().find(|row| row.user == name).map(|row| row.who);
            let here = |name: &str| linked(name).or_else(|| off(name));
            view.workers = pool
                .workers(now)
                .into_iter()
                .map(|row| WorkerRow {
                    worker: row.name.split_once('.').map(|(_, worker)| worker.to_string()).unwrap_or_default(),
                    address: short_address(&format_address(&row.address)),
                    here: here(&row.name),
                    last_share_secs: row.last_share_ms.map(|at| now.saturating_sub(at) / 1_000),
                    name: row.name,
                    speed: row.speed,
                    accepted: row.accepted,
                    refused: row.refused,
                    paid: row.paid,
                    difficulty: row.difficulty,
                    online: row.online,
                })
                .collect();
            let splits = pool.splits();
            view.found = pool
                .blocks()
                .into_iter()
                .map(|block| FoundRow {
                    height: block.height,
                    value: block.value,
                    state: block.state,
                    pool_fee: block.pool_fee,
                    secs_ago: now.saturating_sub(block.found_ms) / 1_000,
                    parts: splits.iter().find(|(height, ..)| *height == block.height).map(|(_, _, split)| split.parts.clone()).unwrap_or_default(),
                })
                .collect();
        }
        self.view = view;
    }
}

impl WorkLink {
    /// Hands the miner the pool's newest work when it changed or grew old.
    fn hand_work(&mut self, who: Who, mining: bool, cx: &mut PoolCx) {
        if who == Who::Own && !mining {
            if self.sent.take().is_some() {
                cx.host.take_work(None);
            }
            return;
        }
        let generation = self.client.generation();
        let fresh = self.sent.is_some_and(|(sent, at)| sent == generation && at.elapsed() < REFRESH_WORK);
        if fresh {
            return;
        }
        self.extranonce2 += 1;
        let Some(work) = self.client.work(self.extranonce2) else {
            return;
        };
        self.sent = Some((generation, Instant::now()));
        let line = work_line(&work.header, work.difficulty);
        match (who, cx.fleet.cloned()) {
            (Who::Own, _) => cx.host.take_work(Some((work.header, work.difficulty))),
            (Who::Node(id), _) => {
                let sent = cx.local.send(id, &line);
                self.set_trouble(sent.is_err().then_some("pool.why_not_running"));
            }
            (Who::Board(id), Some(fleet)) => {
                let trouble = self.trouble.clone();
                thread::spawn(move || {
                    let said = fleet.run_line(id, &line).err().map(|_| "pool.board_refused");
                    if let Ok(mut slot) = trouble.lock() {
                        *slot = said;
                    }
                });
            }
            (Who::Board(_), None) => {}
            (Who::Lent(worker), _) => {
                let sent = lend(worker, Some((&work.header, work.difficulty)), cx);
                self.set_trouble((!sent).then_some("pool.why_heard_only"));
                if sent {
                    self.watch.get_or_insert_with(Watch::start).sent();
                }
            }
        }
    }

    fn set_trouble(&self, said: Option<&'static str>) {
        if let Ok(mut slot) = self.trouble.lock() {
            *slot = said;
        }
    }

    /// Asks a board for its shares now and then, one question at a time, and takes what came.
    fn ask_board(&mut self, id: olc_control::BoardId, fleet: Fleet) -> Vec<BlockHeader> {
        let due = self.asked.is_none_or(|asked| asked.elapsed() >= ASK_BOARDS_EVERY);
        if let (Some(fleet), true, false) = (fleet.cloned(), due, self.asking.load(Ordering::Relaxed)) {
            self.asked = Some(Instant::now());
            self.asking.store(true, Ordering::Relaxed);
            let (asking, found) = (self.asking.clone(), self.found.clone());
            thread::spawn(move || {
                let answer = fleet.ask(id, "work shares").ok().flatten().unwrap_or_default();
                let shares = shares_in(answer.strip_prefix("shares").unwrap_or(""));
                if let Ok(mut found) = found.lock() {
                    found.extend(shares);
                }
                asking.store(false, Ordering::Relaxed);
            });
        }
        self.found.lock().map(|mut found| std::mem::take(&mut *found)).unwrap_or_default()
    }
}
