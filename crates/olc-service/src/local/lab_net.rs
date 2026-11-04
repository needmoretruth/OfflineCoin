//! The Lab's network: participants a person adds one click at a time, each a real node process on
//! the Lab's own coin on 127.0.0.1. Miners hash for real, a pool runs the app's pool code inside
//! its node, payments are signed and checked by every node, an attacker hides a chain and shows it,
//! and the person's own chain and wallet are never touched. One keeper thread owns the processes:
//! the window sends it orders and reads what it publishes.
use super::lab::{lab_coin, place, remove_later};
use super::lab_picture::Picture;
use super::supervisor::{ChainBlock, ChildStatus};
use olc_core::coin::COIN;
use super::{Holding, NodeSpec, Preset, Supervisor, WalletChoice};
use crate::settings::Paths;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

pub use super::lab::{Frame, LabKind, LabState, Mark, Phase, Picture as Drawing, Row, Side};
pub use super::lab_room::{processes_with, Limit, Room, MOST};
pub use super::trouble::Trick;

#[path = "lab_net_add.rs"]
mod add;
#[path = "lab_net_look.rs"]
mod look;
#[path = "lab_net_power.rs"]
mod power;
#[path = "lab_net_roles.rs"]
mod roles;
#[path = "lab_net_runs.rs"]
mod runs;

/// How often the keeper looks at every node and acts.
const TURN: Duration = Duration::from_millis(250);
/// Events kept for the window, which draws the newest.
const KEPT_EVENTS: usize = 240;
const FIRST_ID: u32 = 1_000;
/// Blocks a payment waits under before the receiver counts it as settled.
pub const SETTLED_AFTER: u64 = 2;

/// Each kind of participant a person can add.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    Miner,
    Pool,
    Full,
    Pruned,
    Light,
    Attacker,
    Exploiter,
    Spammer,
    Sender,
    Receiver,
}

impl Kind {
    pub const ALL: [Kind; 10] = [
        Kind::Miner,
        Kind::Pool,
        Kind::Full,
        Kind::Pruned,
        Kind::Light,
        Kind::Sender,
        Kind::Receiver,
        Kind::Spammer,
        Kind::Attacker,
        Kind::Exploiter,
    ];

    pub fn mines(self) -> bool {
        matches!(self, Kind::Miner | Kind::Attacker)
    }

    pub fn holding(self) -> Holding {
        match self {
            Kind::Pruned => Holding::Recent,
            Kind::Light => Holding::Headers,
            _ => Holding::Everything,
        }
    }

    /// Coins it asks a miner for before it can do its part.
    pub fn needs_coins(self) -> u64 {
        match self {
            Kind::Sender => 20 * COIN,
            Kind::Spammer => 100 * COIN,
            Kind::Exploiter => 20 * COIN,
            _ => 0,
        }
    }

    /// What its speed means: hashes a second, payments a minute, or spam payments a second.
    pub fn speed(self) -> Option<Speed> {
        match self {
            Kind::Miner | Kind::Attacker => Some(Speed { least: 1_000, most: 40_000, presets: [4_000, 8_000, 16_000] }),
            Kind::Sender => Some(Speed { least: 2, most: 30, presets: [4, 10, 20] }),
            Kind::Spammer => Some(Speed { least: 2, most: 60, presets: [5, 20, 50] }),
            _ => None,
        }
    }

    /// A short word for the node's name, in its process list and folder.
    fn word(self) -> &'static str {
        match self {
            Kind::Miner => "miner",
            Kind::Pool => "pool",
            Kind::Full => "full",
            Kind::Pruned => "pruned",
            Kind::Light => "light",
            Kind::Attacker => "attacker",
            Kind::Exploiter => "exploiter",
            Kind::Spammer => "spammer",
            Kind::Sender => "sender",
            Kind::Receiver => "receiver",
        }
    }
}

/// The range of a speed and its three presets: slow, normal, fast.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Speed {
    pub least: u64,
    pub most: u64,
    pub presets: [u64; 3],
}

/// One participant as the window sees it.
#[derive(Clone, Debug)]
pub struct Member {
    pub id: u32,
    pub kind: Kind,
    /// Its number among those of its kind, from 1.
    pub number: u32,
    pub speed: u64,
    /// What it really hashes at: its speed, or under the person's limit the same part as all.
    pub limit: u64,
    pub paused: bool,
    /// The pool it works in, for a miner.
    pub pool: Option<u32>,
    /// The members it connected to when it started.
    pub links: Vec<u32>,
    pub status: Option<ChildStatus>,
    pub pid: Option<u32>,
    pub ended: Option<String>,
    /// The key of the one sentence saying what it waits for, if it waits.
    pub waits: Option<&'static str>,
    /// What each trick got from each node, for the bug exploiter.
    pub tricks: Vec<TrickRow>,
    /// It takes part in the experiment running now, so its controls wait.
    pub busy: bool,
    /// A pool turned bad: it keeps its miners' coins and attacks with their power.
    pub bad: bool,
    /// It attacked once and mines honestly since: a person can start it again.
    pub raced: bool,
    /// Payments it received since it was added, counted as they come: its node keeps only the
    /// newest few.
    pub received: Received,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Received {
    pub payments: u64,
    pub settled: u64,
    pub undone: u64,
    /// Payments it made, and coins its pool paid it.
    pub sent: u64,
    pub from_pool: u64,
}

impl Member {
    pub fn address(&self) -> Option<&str> {
        self.status.as_ref().map(|status| status.address.as_str()).filter(|address| !address.is_empty())
    }

    pub fn port(&self) -> Option<u16> {
        self.status.as_ref().and_then(|status| status.port)
    }

    pub fn height(&self) -> u64 {
        self.status.as_ref().map_or(0, |status| status.height)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrickRow {
    pub trick: Trick,
    pub by: u32,
    pub refused: bool,
    /// Refused for the rule the trick breaks.
    pub expected: bool,
    /// The rule the node named, as its chain says it.
    pub rule: String,
    /// Why the trick could not be tried on this node.
    pub failed: Option<String>,
}

/// Something that just happened, for the window to draw once.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Event {
    pub seq: u64,
    pub what: What,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum What {
    /// A block first seen, and the member whose reward it holds.
    Block { finder: Option<u32>, height: u64, id: String, payments: u64 },
    /// A payment signed and sent.
    Paid { from: u32, to: Option<u32>, amount: u64 },
    /// Shares a pool took from a miner since the last look.
    Shares { from: u32, to: u32, count: u64 },
    /// A pool paid a miner its part.
    Payout { from: u32, to: u32, amount: u64 },
    /// A node replaced blocks with those of a chain with more work.
    Replaced { at: u32, blocks: u64 },
    /// A node refused a trick of the bug exploiter `from`, naming the rule as its chain says it.
    Refused { from: u32, by: u32, trick: Trick, rule: String },
    /// A payment to a receiver is under enough blocks.
    Settled { at: u32, amount: u64 },
    /// A payment a receiver held in a block is on no chain any more.
    Undone { at: u32, amount: u64 },
    /// An attack ended: whether the hidden chain won, the attacker's share of all hashing as it
    /// was measured, in percent, and how many blocks behind it gave up.
    Raced { by: u32, won: bool, share: u64, behind: u64 },
}

/// The one-click setups.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scene {
    Race,
    Pool,
    Payment,
    Attack,
    Split,
    Spam,
    Exploit,
}

impl Scene {
    pub const ALL: [Scene; 7] = [Scene::Race, Scene::Pool, Scene::Payment, Scene::Attack, Scene::Split, Scene::Spam, Scene::Exploit];

    /// Who it adds, in order.
    pub fn cast(self) -> &'static [Kind] {
        match self {
            Scene::Race => &[Kind::Miner, Kind::Miner, Kind::Miner],
            Scene::Pool => &[Kind::Miner, Kind::Miner, Kind::Miner, Kind::Pool],
            Scene::Payment => &[Kind::Miner, Kind::Miner, Kind::Sender, Kind::Receiver],
            Scene::Attack => &[Kind::Miner, Kind::Miner, Kind::Receiver, Kind::Attacker, Kind::Attacker],
            Scene::Split => &[Kind::Miner, Kind::Miner, Kind::Miner],
            Scene::Spam => &[Kind::Miner, Kind::Miner, Kind::Sender, Kind::Receiver, Kind::Spammer],
            Scene::Exploit => &[Kind::Miner, Kind::Miner, Kind::Full, Kind::Light, Kind::Exploiter],
        }
    }
}

/// What the person asks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Order {
    Add(Kind),
    Remove(u32),
    Speed(u32, u64),
    Pause(u32, bool),
    /// Puts a miner in a pool, or out of every pool with `None`.
    Join(u32, Option<u32>),
    /// The attacker attacks again, the bug exploiter tries its tricks again.
    Again(u32),
    Clear,
    Scene(Scene),
    /// Stops the attack or split running now; every node is put back as it was.
    StopExperiment,
    /// Takes away the card of the experiment that ended.
    Forget,
    /// Makes this many blocks now with the first miner, as fast as the hash limit lets it.
    MakeBlocks(u64),
    /// The most the Lab's miners may hash together, from the person's limit; 0 is none.
    Budget(u64),
    /// A pool turns bad and attacks with its miners' power, or turns honest again.
    Bad(u32, bool),
    End,
}

/// Everything the window draws from.
#[derive(Clone, Debug, Default)]
pub struct NetState {
    pub members: Vec<Member>,
    pub events: Vec<Event>,
    /// The attack or split running or last run, drawn as the Lab always drew it.
    pub experiment: Option<LabState>,
    /// Blocks made now and asked for.
    pub making: Option<(u64, u64)>,
    pub room: Room,
    pub error: Option<String>,
    pub max_block_bytes: u64,
    /// The most the miners may hash together; 0 is no limit.
    pub budget: u64,
    /// Blocks no chain holds any more, newest last, by id.
    pub replaced: Vec<String>,
}

/// The window's handle on the network. Dropping it stops every process.
pub struct LabNet {
    orders: Sender<Order>,
    state: Arc<Mutex<NetState>>,
    keeper: Option<JoinHandle<()>>,
}

/// Networks alive in this process, so only the first sweeps folders left behind.
static NETS: AtomicUsize = AtomicUsize::new(0);

impl LabNet {
    pub fn start() -> Self {
        let (orders, inbox) = channel();
        let state = Arc::new(Mutex::new(NetState { room: Room::measure(0), ..NetState::default() }));
        let shared = state.clone();
        let sweep = NETS.fetch_add(1, AtomicOrdering::SeqCst) == 0;
        let keeper = thread::spawn(move || {
            let mut keeper = Keeper::new(shared);
            keeper.sweep = sweep;
            keeper.run(&inbox);
        });
        Self { orders, state, keeper: Some(keeper) }
    }

    pub fn order(&self, order: Order) {
        let _ = self.orders.send(order);
    }

    pub fn state(&self) -> NetState {
        self.state.lock().map(|state| state.clone()).unwrap_or_default()
    }
}

impl Drop for LabNet {
    fn drop(&mut self) {
        let _ = self.orders.send(Order::End);
        if let Some(keeper) = self.keeper.take() {
            let _ = keeper.join();
        }
        NETS.fetch_sub(1, AtomicOrdering::SeqCst);
    }
}

/// A payment sent and not answered yet, by the member that pays.
struct Asked {
    to: Option<u32>,
    amount: u64,
}

struct Keeper {
    shared: Arc<Mutex<NetState>>,
    supervisor: Option<Supervisor>,
    coin: Option<PathBuf>,
    members: Vec<Member>,
    specs: BTreeMap<u32, NodeSpec>,
    next_id: u32,
    counts: BTreeMap<Kind, u32>,
    seq: u64,
    events: VecDeque<Event>,
    seen_blocks: BTreeSet<String>,
    top: u64,
    chains: BTreeMap<u32, Vec<ChainBlock>>,
    replaced: VecDeque<String>,
    asked: BTreeMap<u32, VecDeque<Asked>>,
    funded_at: BTreeMap<u32, Instant>,
    paid_at: BTreeMap<u32, Instant>,
    /// The last line of each topic each participant was told, so it is told once.
    told: BTreeMap<(u32, &'static str), String>,
    /// Shares and payouts each pool counted for each miner.
    accepted: BTreeMap<(u32, u32), u64>,
    payouts: BTreeMap<(u32, u32), u64>,
    seen_incoming: BTreeSet<(u32, String)>,
    settled: BTreeSet<(u32, String)>,
    undone: BTreeSet<(u32, String)>,
    runs: runs::Runs,
    /// Bug exploiters that tried their tricks.
    tried: BTreeSet<u32>,
    split_asked: bool,
    scene: Option<Scene>,
    healed_at: Option<Instant>,
    config: Option<olc_core::CoinConfig>,
    /// Folders of networks an earlier app left behind go when the first coin is made.
    sweep: bool,
    room_at: Option<Instant>,
    room: Room,
    error: Option<String>,
    budget: u64,
}

impl Keeper {
    fn new(shared: Arc<Mutex<NetState>>) -> Self {
        Self {
            shared,
            supervisor: None,
            coin: None,
            members: Vec::new(),
            specs: BTreeMap::new(),
            next_id: FIRST_ID,
            counts: BTreeMap::new(),
            seq: 0,
            events: VecDeque::new(),
            seen_blocks: BTreeSet::new(),
            top: 0,
            chains: BTreeMap::new(),
            replaced: VecDeque::new(),
            asked: BTreeMap::new(),
            funded_at: BTreeMap::new(),
            paid_at: BTreeMap::new(),
            told: BTreeMap::new(),
            accepted: BTreeMap::new(),
            payouts: BTreeMap::new(),
            seen_incoming: BTreeSet::new(),
            settled: BTreeSet::new(),
            undone: BTreeSet::new(),
            runs: runs::Runs::default(),
            tried: BTreeSet::new(),
            split_asked: false,
            scene: None,
            healed_at: None,
            config: None,
            sweep: false,
            room_at: None,
            room: Room::measure(0),
            error: None,
            budget: 0,
        }
    }

    fn run(mut self, inbox: &Receiver<Order>) {
        loop {
            let until = Instant::now() + TURN;
            while let Some(left) = until.checked_duration_since(Instant::now()) {
                match inbox.recv_timeout(left) {
                    Ok(Order::End) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                        self.clear();
                        return;
                    }
                    Ok(order) => self.take(order),
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => break,
                }
            }
            self.turn();
        }
    }

    fn take(&mut self, order: Order) {
        self.error = None;
        let done = match order {
            Order::Add(kind) => self.add(kind).map(drop),
            Order::Remove(id) => {
                self.remove(id);
                Ok(())
            }
            Order::Speed(id, speed) => self.set_speed(id, speed),
            Order::Pause(id, paused) => self.pause(id, paused),
            Order::Join(id, pool) => {
                if let Some(member) = self.member_mut(id) {
                    member.pool = pool;
                }
                Ok(())
            }
            Order::Again(id) => self.again(id),
            Order::Clear => {
                self.clear();
                Ok(())
            }
            Order::Scene(scene) => self.scene(scene),
            Order::MakeBlocks(count) => self.make_blocks(count),
            Order::Budget(cap) => {
                self.budget = cap;
                self.limits();
                Ok(())
            }
            Order::Bad(id, bad) => self.turn_bad(id, bad),
            Order::StopExperiment => {
                self.stop_run();
                Ok(())
            }
            Order::Forget => {
                if !self.runs.running() {
                    self.runs.experiment = None;
                }
                Ok(())
            }
            Order::End => Ok(()),
        };
        if let Err(error) = done {
            self.error = Some(error);
        }
        self.publish();
    }

    /// One look at every node, then each one's part, then what the window reads.
    fn turn(&mut self) {
        self.look();
        self.roles();
        self.runs_turn();
        self.publish();
    }

    fn member_mut(&mut self, id: u32) -> Option<&mut Member> {
        self.members.iter_mut().find(|member| member.id == id)
    }

    fn member(&self, id: u32) -> Option<&Member> {
        self.members.iter().find(|member| member.id == id)
    }

    fn event(&mut self, what: What) {
        self.seq += 1;
        self.events.push_back(Event { seq: self.seq, what });
        while self.events.len() > KEPT_EVENTS {
            self.events.pop_front();
        }
    }

    fn send(&mut self, id: u32, line: &str) {
        if let Some(supervisor) = &mut self.supervisor {
            let _ = supervisor.send(id, line);
        }
    }

    fn publish(&mut self) {
        if self.room_at.is_none_or(|at| at.elapsed() > Duration::from_secs(2)) {
            self.room_at = Some(Instant::now());
            self.room = Room::measure(self.members.len());
        }
        let state = NetState {
            members: self.members.clone(),
            events: self.events.iter().cloned().collect(),
            experiment: self.runs.experiment.clone(),
            making: self.runs.making,
            room: self.room,
            error: self.error.clone(),
            max_block_bytes: lab_coin().rules.max_block_bytes,
            budget: self.budget,
            replaced: self.replaced.iter().cloned().collect(),
        };
        if let Ok(mut shared) = self.shared.lock() {
            *shared = state;
        }
    }
}

/// The node of one participant: plain, on 127.0.0.1, with room for the Lab's small chain.
fn spec_of(id: u32, kind: Kind, number: u32, speed: u64) -> NodeSpec {
    NodeSpec {
        name: format!("lab-{}-{number}", kind.word()),
        preset: Preset::Custom,
        hash_cap: if kind.mines() { speed } else { 0 },
        memory_mib: super::lab_room::MEMORY_MIB,
        cpu_percent: 0,
        disk_mib: super::lab_room::DISK_MIB,
        wallet: WalletChoice::Own,
        joined: true,
        listen_lan: false,
        mining: kind.mines(),
        holding: kind.holding(),
        recent: olc_core::chain::FEWEST_RECENT,
        ..Preset::Custom.spec(id)
    }
}

/// A fresh coin and supervisor in a folder of this network's own, so a folder still being
/// removed after "clear all" never holds the next network.
fn fresh_place(sweep: bool) -> Result<(PathBuf, olc_core::CoinConfig, Supervisor), String> {
    let (program, folder) = place()?;
    // Networks an app left behind when it was killed; never one this app still runs.
    let entries = if sweep { std::fs::read_dir(&folder).ok() } else { None };
    if let Some(entries) = entries {
        for entry in entries.flatten() {
            if entry.file_name().to_string_lossy().starts_with("net-") {
                remove_later(entry.path());
            }
        }
    }
    let folder = folder.join(format!("net-{}", crate::core::unix_ms()));
    let coin_folder = folder.join("coins").join("olc-lab");
    std::fs::create_dir_all(&coin_folder).map_err(|error| error.to_string())?;
    let coin = coin_folder.join("coin.json");
    let config = lab_coin();
    std::fs::write(&coin, olc_store::coin_file::to_json(&config)?).map_err(|error| error.to_string())?;
    let supervisor = Supervisor::with_program(Paths { root: folder }, program);
    Ok((coin, config, supervisor))
}

fn forget_folder(coin: Option<PathBuf>) {
    // The coin sits at net-<time>/coins/olc-lab/coin.json.
    if let Some(folder) = coin.as_deref().and_then(|coin| coin.ancestors().nth(3)) {
        remove_later(folder.to_path_buf());
    }
}
