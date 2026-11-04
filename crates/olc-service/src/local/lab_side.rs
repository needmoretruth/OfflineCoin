//! What a node of the Lab's network does beside being a node, inside the node process itself: it
//! runs a pool or works in one with the same pool code the app runs (Stratum on 127.0.0.1), floods
//! small payments, pays at a chosen fee rate, and watches the payments that reach its own wallet.
//! Each order comes as `lab <line>`, and what it does shows in the `lab` part of the status line.
use super::supervisor::ChildStatus;
use super::Supervisor;
use crate::core::{Host, Join, PoolCx, PoolOrder, PoolSeat, PoolSettings, Who};
use crate::settings::Paths;
use olc_core::{Address, OutPoint};
use olc_wallet::{format_address, parse_address};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::net::{IpAddr, Ipv4Addr};
use std::path::PathBuf;
use std::time::Instant;

/// Payments to this wallet the line carries, newest last.
const KEPT_INCOMING: usize = 8;
/// One spam payment: a thousandth of a coin, at the lowest fee rate a payment may pay.
const SPAM_AMOUNT: u64 = 100_000;
const SPAM_RATE: u64 = 1;
/// A spammer splits its coins this many ways first, so its payments need not wait on each other.
const SPAM_SPLIT: u64 = 40;
/// Fewer coins than this to spend from, and the spammer splits again.
const SPAM_FEWEST: usize = 8;
/// Newest blocks whose size the line carries.
const SIZES_SHOWN: u64 = 14;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkerLine {
    /// The address it is paid to.
    pub address: String,
    pub accepted: u64,
    pub paid: u64,
    pub online: bool,
    pub speed: u64,
}

/// A payment to this wallet as this node sees it now.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Incoming {
    pub id: String,
    pub amount: u64,
    /// The height of the block that holds it.
    pub height: Option<u64>,
    /// It was in a block, and now it is in no block and not waiting either: undone.
    pub gone: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LabLine {
    /// Where the pool this node runs listens, on 127.0.0.1.
    pub pool_port: Option<u16>,
    pub workers: Vec<WorkerLine>,
    /// Blocks the pool found, those it no longer lists included.
    pub pool_found: u64,
    /// The pool this node works in.
    pub joined: Option<String>,
    pub working: bool,
    /// Shares the pool took from this node.
    pub accepted: u64,
    pub incoming: Vec<Incoming>,
    /// Bytes of payments waiting for a block.
    pub waiting_bytes: u64,
    /// The oldest block whose body this node still holds.
    pub oldest_held: u64,
    /// Coins unspent on the whole chain, which every node but a light one keeps.
    pub unspent: u64,
    /// The newest blocks as height, bytes and payments.
    pub sizes: Vec<(u64, u64, u64)>,
    pub max_block_bytes: u64,
    pub spam_sent: u64,
}

/// The pool seat of this process, with what it needs around it.
struct Seat {
    seat: PoolSeat,
    settings: PoolSettings,
    local: Supervisor,
}

impl Seat {
    fn new() -> Self {
        let settings = PoolSettings { workers: Vec::new(), ..PoolSettings::default() };
        let local = Supervisor::with_program(Paths { root: PathBuf::new() }, PathBuf::new());
        Self { seat: PoolSeat::default(), settings, local }
    }

    fn cx<'a>(&'a mut self, host: &'a Host) -> (&'a mut PoolSeat, PoolCx<'a>) {
        let cx = PoolCx { host, local: &mut self.local, fleet: None, specs: &[], hellos: &[], settings: &mut self.settings };
        (&mut self.seat, cx)
    }

    fn order(&mut self, host: &Host, order: PoolOrder) {
        let (seat, mut cx) = self.cx(host);
        seat.order(order, &mut cx);
    }

    fn step(&mut self, host: &Host) {
        let (seat, mut cx) = self.cx(host);
        seat.step(&mut cx);
    }
}

#[derive(Default)]
pub struct LabSide {
    on: bool,
    seat: Option<Seat>,
    /// Spam payments a second.
    spam: u64,
    budget: f64,
    turned: Option<Instant>,
    spam_sent: u64,
    incoming: BTreeMap<String, Incoming>,
    order: Vec<String>,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

impl LabSide {
    /// Turns that come faster than the status line: a pool hands out work and takes shares, and a
    /// spammer pays, many times a second.
    pub fn busy(&self) -> bool {
        self.seat.is_some() || self.spam > 0
    }

    /// A `lab` line came, so the status line carries the `lab` part.
    pub fn on(&self) -> bool {
        self.on
    }

    /// Works in a pool, which then takes this node's shares.
    pub fn joined(&self) -> bool {
        self.seat.as_ref().is_some_and(|seat| !seat.settings.joins.is_empty())
    }

    fn seat(&mut self) -> &mut Seat {
        self.seat.get_or_insert_with(Seat::new)
    }

    /// Carries out one `lab` line. A payment answers with the status line, as `pay` does.
    pub fn answer(&mut self, host: &Host, line: &str) -> Option<ChildStatus> {
        let words: Vec<&str> = line.split_whitespace().collect();
        self.on = true;
        match words.as_slice() {
            ["pool", "run", difficulty] => {
                let settings = PoolSettings {
                    port: 0,
                    open: false,
                    stand_in: Some(IpAddr::V4(Ipv4Addr::LOCALHOST)),
                    workers: Vec::new(),
                    fixed_difficulty: difficulty.parse().unwrap_or(0),
                    ..PoolSettings::default()
                };
                self.seat().order(host, PoolOrder::Start(settings));
            }
            ["pool", "join", address, user] => {
                let join = Join { who: Who::Own, address: (*address).to_string(), user: (*user).to_string() };
                self.seat().order(host, PoolOrder::Join(join));
            }
            ["pool", "leave"] => self.seat().order(host, PoolOrder::Leave(Who::Own)),
            ["spam", rate] => self.spam = rate.parse().unwrap_or(0),
            ["connect", address] => host.connect((*address).to_string()),
            ["pay", to, amount, rate] => return Some(pay(host, to, amount, rate)),
            _ => {}
        }
        None
    }

    /// One turn: the pool's, then the spam's, then a look at the payments to this wallet.
    pub fn step(&mut self, host: &Host) {
        if let Some(seat) = &mut self.seat {
            seat.step(host);
        }
        let now = Instant::now();
        let spent = self.turned.map_or(0.0, |turned| now.duration_since(turned).as_secs_f64());
        self.turned = Some(now);
        if self.spam > 0 {
            self.budget = (self.budget + spent * self.spam as f64).min(self.spam as f64);
            self.flood(host);
        } else {
            self.budget = 0.0;
        }
    }

    /// Spends one coin a payment, so no payment waits for another, and splits its coins first.
    fn flood(&mut self, host: &Host) {
        let Ok(mut runner) = host.runner.lock() else {
            return;
        };
        let engine = &mut runner.engine;
        let own = engine.wallet().address();
        let taken: BTreeSet<OutPoint> = engine.pool().iter().flat_map(|tx| tx.inputs.iter().map(|input| input.previous)).collect();
        let next = engine.chain().height() + 1;
        let coins = engine.coins();
        let free: Vec<OutPoint> = coins
            .unspent_of(&own)
            .into_iter()
            .filter(|(point, entry)| !taken.contains(point) && coins.is_mature(entry, next))
            .filter(|(_, entry)| entry.output.value > SPAM_AMOUNT * 4)
            .map(|(point, _)| point)
            .collect();
        if free.len() < SPAM_FEWEST {
            let spendable = engine.spendable();
            let each = spendable / (SPAM_SPLIT + 1);
            if each > SPAM_AMOUNT * 20 && engine.pool().iter().all(|tx| tx.outputs.len() < SPAM_SPLIT as usize) {
                let parts: Vec<(Address, u64)> = (0..SPAM_SPLIT).map(|_| (own, each)).collect();
                let _ = engine.pay_with(&parts, SPAM_RATE, &[]);
            }
        }
        for point in free {
            if self.budget < 1.0 {
                break;
            }
            self.budget -= 1.0;
            if engine.pay_with(&[(own, SPAM_AMOUNT)], SPAM_RATE, &[point]).is_ok() {
                self.spam_sent += 1;
            }
        }
    }

    /// Payments to this wallet: waiting, in a block, or undone when a block that held one left
    /// the chain and the payment did not come back.
    fn watch_incoming(&mut self, host: &Host) {
        let Ok(runner) = host.runner.lock() else {
            return;
        };
        let engine = &runner.engine;
        let chain = engine.chain();
        let wallet = engine.wallet();
        let paid_here = |tx: &olc_core::Tx| -> u64 {
            tx.outputs.iter().filter(|output| wallet.owns(&output.lock)).map(|output| output.value).sum()
        };
        let mut seen: Vec<(String, u64)> = Vec::new();
        for tx in engine.pool() {
            let amount = paid_here(tx);
            // A payment of its own pays its change back: that is no money received.
            if amount > 0 && !tx.inputs.is_empty() && !own_spend(engine, tx) {
                seen.push((hex(&tx.id()), amount));
            }
        }
        let height = chain.height();
        for at in height.saturating_sub(SIZES_SHOWN)..=height {
            let Some(block) = chain.id_at(at).and_then(|id| chain.block(&id)) else {
                continue;
            };
            for tx in block.txs.iter().skip(1) {
                let amount = paid_here(tx);
                if amount > 0 && !own_spend(engine, tx) {
                    seen.push((hex(&tx.id()), amount));
                }
            }
        }
        for (id, amount) in seen {
            if !self.incoming.contains_key(&id) {
                self.order.push(id.clone());
                self.incoming.insert(id.clone(), Incoming { id, amount, height: None, gone: false });
            }
        }
        while self.order.len() > KEPT_INCOMING {
            let old = self.order.remove(0);
            self.incoming.remove(&old);
        }
        for entry in self.incoming.values_mut() {
            let Some(id) = unhash(&entry.id) else {
                continue;
            };
            let now = chain.find_tx(&id);
            if now.is_none() && !engine.in_pool(&id) && entry.height.is_some() {
                entry.gone = true;
            }
            if now.is_some() {
                entry.gone = false;
            }
            entry.height = now;
        }
    }

    /// The `lab` part of the status line, once a `lab` line came.
    pub fn line(&mut self, host: &Host) -> Option<LabLine> {
        if !self.on {
            return None;
        }
        self.watch_incoming(host);
        let mut line = LabLine { spam_sent: self.spam_sent, ..LabLine::default() };
        line.incoming = self.order.iter().filter_map(|id| self.incoming.get(id).cloned()).collect();
        if let Some(seat) = &self.seat {
            let view = seat.seat.view();
            line.pool_port = view.running.then_some(view.port);
            line.workers = view
                .workers
                .iter()
                .map(|row| WorkerLine {
                    address: row.name.split('.').next().unwrap_or_default().to_string(),
                    accepted: row.accepted,
                    paid: row.paid,
                    online: row.online,
                    speed: row.speed,
                })
                .collect();
            line.pool_found = view.let_go.blocks + view.found.len() as u64;
            if let Some(link) = view.links.first() {
                line.joined = Some(link.join.address.clone());
                line.working = link.view.link == olc_pool::Link::Working;
                line.accepted = link.view.accepted;
            }
        }
        if let Ok(runner) = host.runner.lock() {
            let engine = &runner.engine;
            let chain = engine.chain();
            line.waiting_bytes = engine.pool().iter().map(|tx| tx.encode().len() as u64).sum();
            line.oldest_held = chain.oldest_held();
            line.unspent = chain.unspent_count() as u64;
            line.max_block_bytes = engine.rules().max_block_bytes;
            let height = chain.height();
            line.sizes = (height.saturating_sub(SIZES_SHOWN - 1).max(1)..=height)
                .filter_map(|at| {
                    let block = chain.block(&chain.id_at(at)?)?;
                    Some((at, block.size() as u64, block.txs.len().saturating_sub(1) as u64))
                })
                .collect();
        }
        Some(line)
    }
}

/// A payment whose every input is this wallet's own coin: its change, or the spammer's own.
fn own_spend(engine: &olc_node::Engine, tx: &olc_core::Tx) -> bool {
    let wallet = engine.wallet();
    let chain = engine.chain();
    let pool = engine.pool();
    tx.inputs.iter().all(|input| {
        let in_chain = chain.coin_at(&input.previous).map(|entry| wallet.owns(&entry.output.lock));
        let in_pool = || {
            pool.iter()
                .find(|parent| parent.id() == input.previous.txid)
                .and_then(|parent| parent.outputs.get(input.previous.index as usize))
                .map(|output| wallet.owns(&output.lock))
        };
        in_chain.or_else(in_pool).unwrap_or(false)
    })
}

fn unhash(text: &str) -> Option<olc_core::Hash> {
    let mut id = [0u8; 32];
    if text.len() != 64 {
        return None;
    }
    for (at, byte) in id.iter_mut().enumerate() {
        *byte = u8::from_str_radix(text.get(at * 2..at * 2 + 2)?, 16).ok()?;
    }
    Some(id)
}

/// Pays `amount` coins to `to` at `rate` units a byte, and answers as `pay` does.
fn pay(host: &Host, to: &str, amount: &str, rate: &str) -> ChildStatus {
    let paid = match (parse_address(to), crate::core::parse_coins(amount), rate.parse::<u64>()) {
        (Some(to), Some(units), Ok(rate)) => match host.runner.lock() {
            Ok(mut runner) => runner.engine.pay_with(&[(to, units)], rate, &[]).map(|(id, _)| id).map_err(|error| ("notice.not_enough", format!("{error:?}"))),
            Err(_) => Err(("notice.not_enough", String::new())),
        },
        (None, ..) => Err(("notice.bad_address", to.to_string())),
        _ => Err(("notice.bad_amount", amount.to_string())),
    };
    let mut status = ChildStatus::from_view(&host.view());
    match paid {
        Ok(id) => status.paid = Some(hex(&id)),
        Err((key, detail)) => {
            status.error = Some(key.to_string());
            status.detail = detail;
        }
    }
    status
}

/// The address a worker of a lab pool signs in with: its own, and its number.
pub fn worker_name(address: &str, number: u32) -> String {
    let address = parse_address(address).map(|address| format_address(&address)).unwrap_or_default();
    format!("{address}.lab{number}")
}
