//! One node on this computer: its links, its chain on disk and its mining loop.
use super::explorer::{recent_blocks, text_of, BlockRow, Explored};
use super::history::{Activity, ActivityKind, History};
use super::radios::{self, Running};
pub use super::host_parts::Plan;
use super::host_parts::{carry_pool, engine_with, fresh_id, hear_router, listen_somewhere, plugins, stored_bytes, unix_ms, watched_view};
use super::snapshot::{KeyView, NodeView};
use crate::fleet::FleetPort;
use olc_core::{Address, BlockHeader, CoinConfig, Hash, Opened};
use olc_link_tcp::{TcpControl, TcpLink};
use olc_node::{Card, Engine, Hello, Light, Rank, Runner};
use olc_store::{
    hold_folder, hold_wallet, load_wallet, open_chain, save_headers, FolderHold,
    NodeSettings, WalletHold,
};
use olc_wallet::{format_address, WalletError};
use std::collections::VecDeque;
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

const MINING_BATCH: u32 = 5_000;
const IDLE_SLEEP: Duration = Duration::from_millis(5);
const TCP_LINK: usize = 0;
const VIEW_EVERY: Duration = Duration::from_millis(250);
const ASK_EVERY: Duration = Duration::from_secs(20);
/// How often connected nodes are asked how they stand, so their rows stay true.
const ASK_PEERS_EVERY: Duration = Duration::from_secs(2);
const MAX_HEARD: usize = 64;
const HEADERS_SAVED_EVERY: Duration = Duration::from_secs(30);
const STILL_OPENING: &str = "The chain is still loading";

struct Switch {
    coin: CoinConfig,
    folder: PathBuf,
    peers: Vec<String>,
    /// The chain a fork carries on from. It is copied while nothing writes to it.
    keep_from: Option<PathBuf>,
}

/// A payment waiting in the pool, until a block holds it.
pub(super) struct Waiting {
    pub(super) id: Hash,
    pub(super) amount: u64,
    pub(super) time_ms: u64,
}

pub(super) struct State {
    mining: AtomicBool,
    cap: AtomicU64,
    hashrate: AtomicU64,
    stop: AtomicBool,
    switch: Mutex<Option<Switch>>,
    /// A chain is on its way in, and the node's thread says nothing until it is.
    opening: AtomicBool,
    view: Mutex<NodeView>,
    radio_notes: Mutex<Vec<String>>,
    heard: Mutex<VecDeque<(HeardFrom, Hello)>>,
    known: Mutex<Vec<String>>,
    pub(super) waiting: Mutex<Vec<Waiting>>,
    storage: Option<u64>,
    /// The folder of the coin this node runs.
    folder: Mutex<Option<PathBuf>>,
    /// A header from outside to grind in place of mining, with its difficulty.
    work: Mutex<Option<(BlockHeader, u64)>>,
    /// Headers that reached that difficulty, until someone takes them.
    shares: Mutex<Vec<BlockHeader>>,
    /// The last thing a person looked up.
    explored: Mutex<Explored>,
    /// A spend waiting for another signer, and who that is.
    half_spend: Mutex<Option<(String, Vec<String>)>>,
}

pub struct Host {
    pub runner: Arc<Mutex<Runner>>,
    tcp: TcpControl,
    pub(super) state: Arc<State>,
    done: Option<thread::JoinHandle<()>>,
    wallet_folder: PathBuf,
    pub port: Option<u16>,
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

impl Host {
    /// Returns at once. The chain loads on the host's own thread.
    pub fn start(plan: Plan, fleet: Option<Arc<dyn FleetPort>>) -> Result<Self, String> {
        let mut held = None;
        hold_folder(&mut held, &plan.folder)?;
        let wallet_held = hold_wallet(&plan.wallet_folder)?;
        let wallet = load_wallet(&plan.wallet_folder)?;
        let engine = Engine::new(plan.coin.clone(), plugins(), wallet);
        let tcp = TcpLink::new();
        // Boards are reached by USB here. Nothing on the network may speak for one.
        tcp.refuse_control();
        let port = listen_somewhere(&tcp, &plan);
        let control = tcp.control();
        let mut runner = Runner::new(engine);
        runner.set_id(fresh_id()?);
        hear_router(&tcp, &plan, runner.id());
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
            opening: AtomicBool::new(true),
            switch: Mutex::new(Some(Switch {
                coin: plan.coin,
                folder: plan.folder,
                peers: plan.peers,
                keep_from: None,
            })),
            view: Mutex::new(NodeView::default()),
            heard: Mutex::default(),
            radio_notes: Mutex::default(),
            known: Mutex::default(),
            waiting: Mutex::default(),
            storage: plan.storage,
            folder: Mutex::default(),
            work: Mutex::default(),
            shares: Mutex::default(),
            explored: Mutex::default(),
            half_spend: Mutex::default(),
        });
        let worker = Worker {
            runner: runner.clone(),
            state: state.clone(),
            tcp: control.clone(),
            fleet,
            wallet_folder: plan.wallet_folder.clone(),
            history: History::default(),
            port,
            tips: 0,
            last_tip: [0; 32],
            saved_tip: [0; 32],
            saved_at: Instant::now(),
            blocks: Vec::new(),
            radios: Running::default(),
            wants_bluetooth: false,
            held,
            _wallet_held: wallet_held,
        };
        let done = thread::spawn(move || worker.run());
        Ok(Self {
            runner,
            tcp: control,
            state,
            done: Some(done),
            wallet_folder: plan.wallet_folder,
            port,
        })
    }

    /// True from the moment a chain is asked for until it stands in the node. The engine there
    /// now is thrown away then, and whatever was done to it with it.
    pub fn opening(&self) -> bool {
        self.state.opening.load(Ordering::Relaxed)
    }

    /// The running node, for work that must not land on an engine about to be replaced.
    pub(super) fn ready(&self) -> Result<MutexGuard<'_, Runner>, String> {
        let runner = self.runner.lock().map_err(|_| "The node stopped".to_string())?;
        match self.opening() {
            true => Err(STILL_OPENING.to_string()),
            false => Ok(runner),
        }
    }

    pub fn address(&self) -> Address {
        self.runner
            .lock()
            .map(|runner| runner.engine.address())
            .unwrap_or_default()
    }

    pub(super) fn wallet_folder(&self) -> PathBuf {
        self.wallet_folder.clone()
    }

    pub(super) fn chain_folder(&self) -> Option<PathBuf> {
        self.state.folder.lock().ok().and_then(|folder| folder.clone())
    }

    pub(super) fn set_half_spend(&self, half: Option<(String, Vec<String>)>) {
        if let Ok(mut slot) = self.state.half_spend.lock() {
            *slot = half;
        }
    }

    pub(super) fn set_explored(&self, explored: Explored) {
        if let Ok(mut slot) = self.state.explored.lock() {
            *slot = explored;
        }
    }

    pub(super) fn set_work(&self, work: Option<(BlockHeader, u64)>) {
        if let Ok(mut slot) = self.state.work.lock() {
            *slot = work;
        }
    }

    pub fn take_shares(&self) -> Vec<BlockHeader> {
        self.state
            .shares
            .lock()
            .map(|mut shares| shares.drain(..).collect())
            .unwrap_or_default()
    }

    pub fn view(&self) -> NodeView {
        let view = self.state.view.lock().map(|view| view.clone()).unwrap_or_default();
        NodeView { loading: self.state.opening.load(Ordering::Relaxed), ..view }
    }

    /// Radios the settings asked for that would not start.
    pub fn take_radio_notes(&self) -> Vec<String> {
        self.state.radio_notes.lock().map(|mut notes| notes.drain(..).collect()).unwrap_or_default()
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
    pub fn switch(&self, coin: CoinConfig, folder: PathBuf, peers: Vec<String>, keep_from: Option<PathBuf>) {
        if let Ok(mut switch) = self.state.switch.lock() {
            // Said before the thread can see it, so nothing lands on the old engine from here on.
            self.state.opening.store(true, Ordering::Relaxed);
            *switch = Some(Switch { coin, folder, peers, keep_from });
        }
    }

    /// Keeps a way to reach a node. The node dials it when nothing else reaches that node.
    pub fn connect(&self, address: String) {
        if let Ok(mut runner) = self.runner.lock() {
            runner.add_address(TCP_LINK, address);
        }
    }

    /// Takes connections on another host, such as only this computer, on the same port.
    pub fn listen_on(&self, host: &str) -> Result<(), String> {
        let port = self.port.ok_or("This app is not taking connections.")?;
        self.tcp.listen_again(&format!("{host}:{port}")).map(|_| ()).map_err(|error| error.to_string())
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
    wallet_folder: PathBuf,
    history: History,
    port: Option<u16>,
    tips: u64,
    last_tip: Hash,
    saved_tip: Hash,
    saved_at: Instant,
    blocks: Vec<BlockRow>,
    radios: Running,
    wants_bluetooth: bool,
    held: Option<FolderHold>,
    _wallet_held: WalletHold,
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
                if self.ground(&runner, batch) {
                    hashes += u64::from(batch);
                } else if mining && runner.engine.syncing_to().is_none() {
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

    /// Tries a batch of work from outside, when there is any.
    fn ground(&self, runner: &Runner, batch: u32) -> bool {
        let Ok(mut slot) = self.state.work.lock() else {
            return false;
        };
        let Some((header, difficulty)) = slot.as_mut() else {
            return false;
        };
        if runner.engine.grind(header, *difficulty, batch) {
            if let Ok(mut shares) = self.state.shares.lock() {
                shares.push(*header);
            }
            header.nonce = header.nonce.wrapping_add(1);
        }
        true
    }

    fn keep(&mut self, runner: &mut Runner, asked: &mut Asked) {
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

    /// Moves the node to the chain a person chose. The new chain opens before the old one goes,
    /// so a chain that cannot open leaves the node where it was.
    fn take_switch(&mut self) {
        let switch = self.state.switch.lock().ok().and_then(|mut slot| slot.take());
        if let Some(switch) = switch {
            self.state.opening.store(true, Ordering::Relaxed);
            self.open_switch(switch);
            // Under the same lock as `Host::switch`, so a chain asked for just now keeps its word.
            if let Ok(next) = self.state.switch.lock() {
                self.state.opening.store(next.is_some(), Ordering::Relaxed);
            }
        }
    }

    fn open_switch(&mut self, switch: Switch) {
        let Ok(wallet) = load_wallet(&self.wallet_folder) else {
            return;
        };
        #[cfg(test)]
        super::ops_tests::wait_at_gate(&switch.folder);
        let held = hold_folder(&mut self.held, &switch.folder);
        if let Some(from) = switch.keep_from.as_ref().filter(|_| held.is_ok()) {
            let _ = super::coins::copy_chain(from, &switch.folder);
        }
        let opened = held.and_then(|()| {
            open_chain(&switch.folder, switch.coin, plugins(), self.state.storage)
        });
        let chain = match opened {
            Ok(chain) => chain,
            Err(error) => {
                eprintln!("{error}");
                return;
            }
        };
        let settings = NodeSettings::load(&switch.folder);
        let engine = engine_with(Engine::on_chain(chain, wallet), &settings, &switch.folder);
        if let Ok(mut folder) = self.state.folder.lock() {
            *folder = Some(switch.folder);
        }
        self.history = History::default();
        self.last_tip = [0; 32];
        self.tcp.drop_peers();
        if let (Ok(mut runner), Some(rank)) = (self.runner.lock(), Rank::parse(&settings.links)) {
            runner.set_rank(rank);
        }
        if let Ok(mut runner) = self.runner.lock() {
            self.wants_bluetooth = settings.bluetooth;
            let problems = self.radios.bring_up(&mut runner, &settings);
            if let Ok(mut notes) = self.state.radio_notes.lock() {
                notes.extend(problems);
            }
            carry_pool(&mut runner, engine);
            runner.forget_addresses();
            for peer in switch.peers {
                runner.add_address(TCP_LINK, peer);
            }
        }
    }

    /// Keeps a light node's headers for the next start, now and then.
    fn save_light(&mut self, engine: &Engine, tip: &Hash) {
        if self.saved_tip == *tip || self.saved_at.elapsed() < HEADERS_SAVED_EVERY {
            return;
        }
        let folder = self.state.folder.lock().ok().and_then(|folder| folder.clone());
        let headers = engine.light().and_then(|light| light.headers().saved());
        if let (Some(folder), Some(headers)) = (folder, headers) {
            self.saved_tip = *tip;
            self.saved_at = Instant::now();
            // The next save tries again, and peers can give back whatever is lost.
            let _ = save_headers(&folder, headers);
        }
    }

    fn publish(&mut self, runner: &mut Runner) {
        let engine = &runner.engine;
        let chain = engine.chain();
        let status = engine.status();
        let wallet = engine.wallet();
        let own: Vec<Address> = wallet.keys().map(|(address, _)| address).collect();
        self.history.follow(chain, &own);
        if status.tip != self.last_tip {
            self.last_tip = status.tip;
            self.tips += 1;
            self.blocks = recent_blocks(chain);
        }
        self.save_light(engine, &status.tip);
        let coins = engine.coins();
        let next = coins.height() + 1;
        let mine: Vec<_> = own.iter().flat_map(|address| coins.unspent_of(address)).collect();
        let held = |mature: bool| -> u64 {
            let ripe = mine.iter().filter(|(_, entry)| coins.is_mature(entry, next) == mature);
            ripe.map(|(_, entry)| entry.output.value).sum()
        };
        let (spendable, maturing) = (held(true), held(false));
        let receiving = wallet.receiving();
        let keys = wallet.keys().enumerate().map(|(index, (address, public))| KeyView {
            address: format_address(&address),
            public: public.iter().map(|byte| format!("{byte:02x}")).collect(),
            receiving: index == receiving,
        });
        let held = chain.held();
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
            keeps: engine.keeps(),
            peers: runner.neighbour_count(),
            pool: status.pool,
            hashrate: self.state.hashrate.load(Ordering::Relaxed),
            cap: self.state.cap.load(Ordering::Relaxed),
            mining: self.state.mining.load(Ordering::Relaxed) && engine.light().is_none(),
            light: engine.light().map(Light::status),
            watched: watched_view(engine),
            syncing_to: status.syncing_to,
            loading: false,
            balance: status.balance,
            spendable,
            maturing,
            address: format_address(&engine.address()),
            activity,
            blocks_mined: self.history.blocks_mined(),
            port: self.port,
            tip_changes: self.tips,
            stored_bytes: stored_bytes(engine, held.bytes),
            storage_full: held.oldest > 0,
            keys: keys.collect(),
            miner: format_address(&engine.miner()),
            miner_is_chosen: engine.miner_is_chosen(),
            mine_note: text_of(engine.note()),
            merging: engine.merging(),
            version_one: engine.version_one(),
            links: runner.rank(),
            bluetooth: radios::bluetooth_here().then_some(self.wants_bluetooth),
            no_bluetooth: radios::no_bluetooth(),
            oldest: held.oldest,
            recent: held.recent,
            fits: held.fits,
            unspent: chain.unspent_count(),
            needs_history: status.needs_history,
            assumed_from: status.assumed_from,
            fetching: status.fetching,
            rebuilt: matches!(chain.opened(), Opened::Rebuilt { .. } | Opened::Lost),
            blocks: self.blocks.clone(),
            notes: self.history.notes().cloned().collect(),
            explored: self.state.explored.lock().map(|x| x.clone()).unwrap_or_default(),
            locks: engine.locked_coins(),
            half_spend: self.state.half_spend.lock().map(|x| x.clone()).unwrap_or_default(),
            wallet: super::host_wallet::wallet_view(engine),
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
