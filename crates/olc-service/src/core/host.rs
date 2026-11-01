//! One node on this computer: its links, its chain on disk and its mining loop.
use super::history::{Activity, ActivityKind, History};
use super::snapshot::NodeView;
use crate::fleet::FleetPort;
use olc_core::{Address, CoinConfig, Hash, Plugins};
use olc_link_tcp::{TcpControl, TcpLink};
use olc_node::{Card, Engine, Hello, Runner};
use olc_pow::ProofOfWork;
use olc_store::{load_or_create_secret, Store};
use olc_wallet::{format_address, parse_address, Wallet, WalletError};
use std::collections::VecDeque;
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const MINING_BATCH: u32 = 5_000;
const IDLE_SLEEP: Duration = Duration::from_millis(5);
const TCP_LINK: usize = 0;
const VIEW_EVERY: Duration = Duration::from_millis(250);
const ASK_EVERY: Duration = Duration::from_secs(20);
/// How often connected nodes are asked how they stand, so their rows stay true.
const ASK_PEERS_EVERY: Duration = Duration::from_secs(2);
const RESTORE_STEP: usize = 64;
const MAX_HEARD: usize = 64;

pub fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |time| time.as_millis() as u64)
}

/// How many hashes this computer does in a second on one core.
pub fn full_rate() -> u64 {
    const ROUNDS: u32 = 20_000;
    let started = Instant::now();
    let mut seed = [0u8; 80];
    for _ in 0..ROUNDS {
        let digest = olc_crypto::hash(&seed);
        seed[..32].copy_from_slice(&digest);
    }
    let micros = started.elapsed().as_micros().max(1) as u64;
    u64::from(ROUNDS) * 1_000_000 / micros
}

pub struct Plan {
    pub coin: CoinConfig,
    pub folder: PathBuf,
    pub wallet_folder: PathBuf,
    pub bind_host: String,
    pub port: u16,
    pub port_tries: u16,
    pub peers: Vec<String>,
    pub cap: u64,
    pub mining: bool,
    pub card: Card,
    /// Bytes the chain may take on disk.
    pub storage: Option<u64>,
}

struct Switch {
    coin: CoinConfig,
    folder: PathBuf,
    peers: Vec<String>,
}

/// A payment waiting in the pool, until a block holds it.
struct Waiting {
    id: Hash,
    amount: u64,
    time_ms: u64,
}

struct State {
    mining: AtomicBool,
    cap: AtomicU64,
    hashrate: AtomicU64,
    stop: AtomicBool,
    switch: Mutex<Option<Switch>>,
    view: Mutex<NodeView>,
    heard: Mutex<VecDeque<(HeardFrom, Hello)>>,
    known: Mutex<Vec<String>>,
    waiting: Mutex<Vec<Waiting>>,
    storage: Option<u64>,
}

pub struct Host {
    pub runner: Arc<Mutex<Runner>>,
    tcp: TcpControl,
    state: Arc<State>,
    done: Option<thread::JoinHandle<()>>,
    address: Address,
    pub port: Option<u16>,
}

fn new_engine(coin: CoinConfig, secret: [u8; 32]) -> Result<Engine, String> {
    let wallet = Wallet::from_secret(secret).map_err(|_| "The key file holds a bad key")?;
    let plugins = Plugins {
        consensus: Box::new(ProofOfWork),
        languages: olc_lang::all(),
    };
    Ok(Engine::new(coin, plugins, wallet))
}

/// Who said a thing, as far as this node can tell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeardFrom {
    Network(IpAddr),
    /// A link this app opened itself.
    Board,
}

struct Asked {
    all: Instant,
    peers: Instant,
}

fn listen_somewhere(tcp: &TcpLink, plan: &Plan) -> Option<u16> {
    (0..plan.port_tries.max(1))
        .filter_map(|step| plan.port.checked_add(step))
        .find_map(|port| tcp.listen(&format!("{}:{port}", plan.bind_host)).ok())
        .map(|address| address.port())
}

impl Host {
    /// Returns at once. The chain loads on the host's own thread.
    pub fn start(plan: Plan, fleet: Option<Arc<dyn FleetPort>>) -> Result<Self, String> {
        let secret = load_or_create_secret(&plan.wallet_folder)?;
        let engine = new_engine(plan.coin.clone(), secret)?;
        let address = engine.address();
        let tcp = TcpLink::new();
        // Boards are reached by USB here. Nothing on the network may speak for one.
        tcp.refuse_control();
        let port = listen_somewhere(&tcp, &plan);
        let control = tcp.control();
        let mut runner = Runner::new(engine);
        runner.card = Card {
            port: port.unwrap_or(0),
            ..plan.card.clone()
        };
        let link = runner.add_link(Box::new(tcp));
        debug_assert_eq!(link, TCP_LINK);
        let runner = Arc::new(Mutex::new(runner));
        let state = Arc::new(State {
            mining: AtomicBool::new(plan.mining),
            cap: AtomicU64::new(plan.cap),
            hashrate: AtomicU64::new(0),
            stop: AtomicBool::new(false),
            switch: Mutex::new(Some(Switch {
                coin: plan.coin,
                folder: plan.folder,
                peers: plan.peers,
            })),
            view: Mutex::new(NodeView::default()),
            heard: Mutex::default(),
            known: Mutex::default(),
            waiting: Mutex::default(),
            storage: plan.storage,
        });
        let worker = Worker {
            runner: runner.clone(),
            state: state.clone(),
            tcp: control.clone(),
            fleet,
            secret,
            address,
            store: None,
            history: History::default(),
            port,
            tips: 0,
            last_tip: [0; 32],
        };
        let done = thread::spawn(move || worker.run());
        Ok(Self {
            runner,
            tcp: control,
            state,
            done: Some(done),
            address,
            port,
        })
    }

    pub fn address(&self) -> Address {
        self.address
    }

    pub fn view(&self) -> NodeView {
        self.state
            .view
            .lock()
            .map(|view| view.clone())
            .unwrap_or_default()
    }

    pub fn take_heard(&self) -> Vec<(HeardFrom, Hello)> {
        self.state
            .heard
            .lock()
            .map(|mut heard| heard.drain(..).collect())
            .unwrap_or_default()
    }

    pub fn set_known(&self, known: Vec<String>) {
        if let Ok(mut slot) = self.state.known.lock() {
            *slot = known;
        }
    }

    pub fn set_mining(&self, on: bool) {
        self.state.mining.store(on, Ordering::Relaxed);
    }

    pub fn set_cap(&self, cap: u64) {
        self.state.cap.store(cap, Ordering::Relaxed);
    }

    /// Moves this node to another coin, which keeps the same links.
    pub fn switch(&self, coin: CoinConfig, folder: PathBuf, peers: Vec<String>) {
        if let Ok(mut switch) = self.state.switch.lock() {
            *switch = Some(Switch {
                coin,
                folder,
                peers,
            });
        }
    }

    pub fn connect(&self, address: String) {
        self.tcp.connect(address);
    }

    /// Pays at the lowest fee the coin allows.
    pub fn pay(&self, to: &str, amount: u64) -> Result<Hash, PayError> {
        let to = parse_address(to.trim()).ok_or(PayError::BadAddress)?;
        let mut runner = self.runner.lock().map_err(|_| PayError::Stopped)?;
        let fee = runner.engine.chain().rules().min_fee;
        let id = runner.engine.pay(to, amount, fee).map_err(PayError::Wallet)?;
        if let Ok(mut waiting) = self.state.waiting.lock() {
            let time_ms = unix_ms();
            waiting.push(Waiting {
                id,
                amount,
                time_ms,
            });
        }
        Ok(id)
    }
}

#[derive(Debug)]
pub enum PayError {
    BadAddress,
    Stopped,
    Wallet(WalletError),
}

impl Drop for Host {
    fn drop(&mut self) {
        self.state.stop.store(true, Ordering::Relaxed);
        self.tcp.close();
        if let Some(done) = self.done.take() {
            let _ = done.join();
        }
    }
}

/// The host's thread.
struct Worker {
    runner: Arc<Mutex<Runner>>,
    state: Arc<State>,
    tcp: TcpControl,
    fleet: Option<Arc<dyn FleetPort>>,
    secret: [u8; 32],
    address: Address,
    store: Option<Store>,
    history: History,
    port: Option<u16>,
    tips: u64,
    last_tip: Hash,
}

impl Worker {
    fn run(mut self) {
        let mut window = Instant::now();
        let mut hashes = 0u64;
        let mut viewed = Instant::now();
        let mut asked = Asked {
            all: Instant::now(),
            peers: Instant::now(),
        };
        while !self.state.stop.load(Ordering::Relaxed) {
            self.take_switch();
            let cap = self.state.cap.load(Ordering::Relaxed);
            // A batch bigger than the cap allows in a window would overrun it.
            let batch = u32::try_from(cap / 100)
                .map_or(MINING_BATCH, |share| share.clamp(1, MINING_BATCH));
            let batch = if cap == 0 { MINING_BATCH } else { batch };
            let elapsed_ms = window.elapsed().as_millis() as u64;
            let allowed = cap == 0 || hashes < cap * elapsed_ms / 1_000;
            let mining = self.state.mining.load(Ordering::Relaxed) && allowed;
            let shared = self.runner.clone();
            if let Ok(mut runner) = shared.lock() {
                let now = unix_ms();
                runner.step(now);
                // A node catching up does not mine, so it counts nothing either.
                if mining && runner.engine.syncing_to().is_none() {
                    runner.engine.mine(batch, now);
                    hashes += u64::from(batch);
                }
                if let Some(fleet) = &self.fleet {
                    fleet.pump(&mut runner);
                }
                self.keep(&mut runner, &mut asked);
                if viewed.elapsed() >= VIEW_EVERY {
                    viewed = Instant::now();
                    self.publish(&mut runner);
                }
            }
            if elapsed_ms >= 1_000 {
                let rate = hashes * 1_000 / elapsed_ms;
                self.state.hashrate.store(rate, Ordering::Relaxed);
                hashes = 0;
                window = Instant::now();
            }
            if mining {
                // Lets others take the lock between batches.
                thread::yield_now();
            } else {
                thread::sleep(IDLE_SLEEP);
            }
        }
    }

    fn keep(&mut self, runner: &mut Runner, asked: &mut Asked) {
        while let Some(block) = runner.engine.take_fresh_block() {
            if let Some(store) = &mut self.store {
                store.append(&block);
            }
        }
        while runner.engine.take_event().is_some() {}
        while let Some((peer, hello)) = runner.take_hello() {
            if let Ok(mut heard) = self.state.heard.lock() {
                if heard.len() >= MAX_HEARD {
                    heard.pop_front();
                }
                // Link 0 is TCP. A socket gone before its address is read says nothing: only a
                // link this app opened may speak for a board.
                let number = Runner::number_on_link(peer);
                let from = match Runner::link_of(peer) {
                    TCP_LINK => self.tcp.ip_of(number).map(HeardFrom::Network),
                    _ => Some(HeardFrom::Board),
                };
                if let Some(from) = from {
                    heard.push_back((from, hello));
                }
            }
        }
        if asked.all.elapsed() >= ASK_EVERY {
            asked.all = Instant::now();
            asked.peers = asked.all;
            runner.ask_who();
        } else if asked.peers.elapsed() >= ASK_PEERS_EVERY {
            asked.peers = Instant::now();
            runner.ask_who_on(TCP_LINK);
        }
    }

    /// Loads the chain in small steps, so the node keeps its lock free and the screen can count.
    fn take_switch(&mut self) {
        let switch = self.state.switch.lock().ok().and_then(|mut slot| slot.take());
        let Some(switch) = switch else {
            return;
        };
        let (mut store, saved) = match Store::open(&switch.folder) {
            Ok(opened) => opened,
            Err(error) => {
                eprintln!("{error}");
                return;
            }
        };
        let Ok(engine) = new_engine(switch.coin, self.secret) else {
            return;
        };
        store.set_limit(self.state.storage);
        self.store = Some(store);
        self.history = History::default();
        self.last_tip = [0; 32];
        self.tcp.drop_peers();
        if let Ok(mut runner) = self.runner.lock() {
            runner.set_engine(engine);
        }
        let total = saved.len();
        for (index, chunk) in saved.chunks(RESTORE_STEP).enumerate() {
            if self.state.stop.load(Ordering::Relaxed) {
                return;
            }
            if let Ok(mut runner) = self.runner.lock() {
                chunk
                    .iter()
                    .for_each(|block| runner.engine.restore_block(block.clone()));
            }
            if let Ok(mut view) = self.state.view.lock() {
                view.loading = Some(((index + 1) * RESTORE_STEP, total));
            }
        }
        for peer in switch.peers {
            self.tcp.connect(peer);
        }
    }

    fn publish(&mut self, runner: &mut Runner) {
        let engine = &runner.engine;
        let chain = engine.chain();
        let status = engine.status();
        self.history.follow(chain, &self.address);
        if status.tip != self.last_tip {
            self.last_tip = status.tip;
            self.tips += 1;
        }
        let next = chain.height() + 1;
        let spendable = chain
            .unspent_of(&self.address)
            .iter()
            .filter(|(_, entry)| chain.is_mature(entry, next))
            .map(|(_, entry)| entry.output.value)
            .sum();
        let mut activity = Vec::new();
        if let Ok(mut waiting) = self.state.waiting.lock() {
            waiting.retain(|item| engine.in_pool(&item.id));
            activity.extend(waiting.iter().rev().map(|item| Activity {
                kind: ActivityKind::Sent,
                height: None,
                amount: item.amount,
                time_ms: item.time_ms,
                id: item.id,
            }));
        }
        activity.extend(self.history.items().cloned());
        let view = NodeView {
            coin: Some(chain.config().clone()),
            genesis: chain.genesis_id(),
            tip: status.tip,
            height: status.height,
            work: status.work,
            difficulty: status.difficulty,
            // The engine remembers every peer it has heard. This counts those still connected.
            peers: self.tcp.count() + status.peers.saturating_sub(runner.peers_on(0)),
            pool: status.pool,
            hashrate: self.state.hashrate.load(Ordering::Relaxed),
            cap: self.state.cap.load(Ordering::Relaxed),
            mining: self.state.mining.load(Ordering::Relaxed),
            syncing_to: status.syncing_to,
            loading: None,
            balance: status.balance,
            spendable,
            address: format_address(&self.address),
            activity,
            blocks_mined: self.history.blocks_mined(),
            port: self.port,
            tip_changes: self.tips,
            stored_bytes: self.store.as_ref().map_or(0, Store::written),
            storage_full: self.store.as_ref().is_some_and(Store::is_full),
        };
        runner.card.mining = view.mining;
        runner.card.hashrate = view.hashrate;
        if let Ok(known) = self.state.known.lock() {
            runner.card.known.clone_from(&known);
        }
        if let Ok(mut slot) = self.state.view.lock() {
            *slot = view;
        }
    }
}
