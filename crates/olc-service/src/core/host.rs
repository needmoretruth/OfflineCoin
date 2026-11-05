//! One node on this computer: its links, its chain on disk and its mining loop.
use super::explorer::{BlockRow, Explored};
use super::history::History;
use super::radios::Running;
pub use super::host_parts::Plan;
use super::host_parts::{carry_pool, engine_with, fresh_id, hear_router, listen_somewhere, plugins, unix_ms};
use super::snapshot::NodeView;
use crate::fleet::FleetPort;
use olc_core::{Address, BlockHeader, CoinConfig, Hash};
use olc_link_tcp::{TcpControl, TcpLink};
use olc_node::{Card, Engine, Hello, Rank, Runner};
use olc_store::{
    hold_folder, hold_wallet, load_wallet, open_chain, save_headers, FolderHold,
    NodeSettings, WalletHold,
};
use olc_wallet::WalletError;
use std::collections::VecDeque;
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

mod mine;
mod speed;
mod view;

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
    /// Threads that mine. One is the node's own thread.
    threads: AtomicUsize,
    /// Mining rests once the chain is this high, or never when 0: `generate` makes no more than asked.
    mine_to: AtomicU64,
    hashrate: AtomicU64,
    /// The cap the speed above was read under.
    hashrate_cap: AtomicU64,
    /// The threads the crew runs now; 0 while mining is off.
    hashing_threads: AtomicUsize,
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
    pub(super) folder: Mutex<Option<PathBuf>>,
    /// A header from outside to grind in place of mining, with its difficulty.
    work: Mutex<Option<(BlockHeader, u64)>>,
    /// Addresses a person added since the last switch: its list was made before them.
    by_hand: Mutex<Vec<String>>,
    /// Headers that reached that difficulty, until someone takes them.
    shares: Mutex<Vec<BlockHeader>>,
    /// The last thing a person looked up.
    explored: Mutex<Explored>,
    /// A spend waiting for another signer, and who that is.
    half_spend: Mutex<Option<(String, Vec<String>)>>,
    pub(super) channels: Mutex<super::host_channels::ChannelDesk>,
    /// The network link's own controls: who is connected, dropping one, keeping an address out.
    pub(super) tcp: TcpControl,
    /// Tips other miners hold, where the node looks for a branch racing its own.
    other_tips: Mutex<Vec<Hash>>,
    /// The wallet's password: whether it has one, and the seal while it is open.
    pub(super) purse: Mutex<super::wallet_lock::LockState>,
    /// What this node said of blocks and payments handed to it, with a running count. The Lab's
    /// bug exploiter reads the newest from the status line; the screen ignores them.
    pub(super) verdicts: Mutex<(u64, VecDeque<super::snapshot::Verdict>)>,
}

/// The newest verdicts kept, so a reader looking once a second catches each one.
const KEPT_VERDICTS: usize = 32;

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
    /// A link this app opened itself, by its number.
    Board(usize),
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
        let wallet_held = (!plan.shares_wallet).then(|| hold_wallet(&plan.wallet_folder)).transpose()?;
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
            threads: AtomicUsize::new(1),
            mine_to: AtomicU64::new(0),
            hashrate: AtomicU64::new(0),
            hashrate_cap: AtomicU64::new(0),
            hashing_threads: AtomicUsize::new(0),
            stop: AtomicBool::new(false),
            opening: AtomicBool::new(true),
            switch: Mutex::new(Some(Switch {
                coin: plan.coin,
                folder: plan.folder,
                peers: plan.peers,
                keep_from: None,
            })),
            view: Mutex::new(NodeView::default()),
            tcp: control.clone(),
            heard: Mutex::default(),
            radio_notes: Mutex::default(),
            known: Mutex::default(),
            waiting: Mutex::default(),
            storage: plan.storage,
            folder: Mutex::default(),
            work: Mutex::default(),
            by_hand: Mutex::default(),
            shares: Mutex::default(),
            explored: Mutex::default(),
            half_spend: Mutex::default(),
            channels: Mutex::default(),
            other_tips: Mutex::default(),
            purse: Mutex::new(super::wallet_lock::LockState::read(&plan.wallet_folder)),
            verdicts: Mutex::default(),
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
            watch: super::chain_view::ChainWatch::default(),
            radios: Running::default(),
            wants_bluetooth: false,
            held,
            _wallet_held: wallet_held,
            crew: mine::Crew::default(),
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

    /// The tips other miners say they hold, for the node to look for a race at its top.
    pub fn watch_tips(&self, tips: Vec<Hash>) {
        if let Ok(mut slot) = self.state.other_tips.lock() {
            *slot = tips;
        }
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

    /// The threads its crew runs now, as it last took the count: 0 while mining is off.
    pub fn hashing_threads(&self) -> usize {
        self.state.hashing_threads.load(Ordering::Relaxed)
    }

    /// The threads it was told to mine on.
    pub fn threads(&self) -> usize {
        self.state.threads.load(Ordering::Relaxed)
    }

    /// Mining rests once the chain reaches `height`; `None` lifts it.
    pub fn mine_to(&self, height: Option<u64>) {
        self.state.mine_to.store(height.unwrap_or(0), Ordering::Relaxed);
    }

    /// Mines on this many threads, under the same cap.
    pub fn set_threads(&self, threads: usize) {
        self.state.threads.store(threads.max(1), Ordering::Relaxed);
    }

    /// Moves this node to another coin, which keeps the same links.
    pub fn switch(&self, coin: CoinConfig, folder: PathBuf, peers: Vec<String>, keep_from: Option<PathBuf>) {
        if let Ok(mut switch) = self.state.switch.lock() {
            // Said before the thread can see it, so nothing lands on the old engine from here on.
            self.state.opening.store(true, Ordering::Relaxed);
            *switch = Some(Switch { coin, folder, peers, keep_from });
            // The list given here holds every address so far.
            if let Ok(mut by_hand) = self.state.by_hand.lock() {
                by_hand.clear();
            }
        }
    }

    /// Keeps a way to reach a node. The node dials it when nothing else reaches that node.
    pub fn connect(&self, address: String) {
        // Kept first, so a chain that opens now and drops every address dials it again.
        if let Ok(mut by_hand) = self.state.by_hand.lock() {
            if !by_hand.contains(&address) {
                by_hand.push(address.clone());
            }
        }
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
    watch: super::chain_view::ChainWatch,
    radios: Running,
    wants_bluetooth: bool,
    held: Option<FolderHold>,
    _wallet_held: Option<WalletHold>,
    crew: mine::Crew,
}

impl Worker {
    fn run(mut self) {
        let mut viewed = Instant::now();
        let mut asked = Asked {
            all: Instant::now(),
            peers: Instant::now(),
        };
        while !self.state.stop.load(Ordering::Relaxed) {
            self.take_switch();
            let cap = self.state.cap.load(Ordering::Relaxed);
            let threads = self.state.threads.load(Ordering::Relaxed);
            let wanted = self.state.mining.load(Ordering::Relaxed);
            // Mining runs on threads of its own, even on one, so this thread holds the chain only
            // to build a block and add it, and links and reports never wait behind the hashing.
            let outside = self.state.work.lock().ok().and_then(|work| *work);
            self.crew.resize(if wanted || outside.is_some() { threads.max(1) } else { 0 });
            self.state.hashing_threads.store(self.crew.size(), Ordering::Relaxed);
            self.crew.set_cap(cap);
            let shared = self.runner.clone();
            if let Ok(mut runner) = shared.lock() {
                let now = unix_ms();
                // First, so the blocks the channel watcher has yet to read are held before any
                // block can connect, from the first turn after a start on.
                super::host_channels::pump(&self.state, &mut runner);
                runner.step(now);
                // Work from outside goes first, on every thread; a node catching up does not mine.
                let mine_to = self.state.mine_to.load(Ordering::Relaxed);
                let below = mine_to == 0 || runner.engine.chain().height() < mine_to;
                let free = !runner.engine.catching_up() && runner.engine.light().is_none() && below;
                if let Some((header, difficulty)) = outside {
                    let found = self.crew.follow(header, difficulty);
                    if let Ok(mut shares) = self.state.shares.lock() {
                        shares.extend(found);
                    }
                } else if wanted && free {
                    self.crew.lead(&mut runner.engine, now);
                } else {
                    self.crew.rest();
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
            // The speed over the last five seconds, read from the cap's own windows, so a reading
            // never stands above the cap the crew keeps to.
            self.state.hashrate.store(self.crew.speed(), Ordering::Relaxed);
            self.state.hashrate_cap.store(cap, Ordering::Relaxed);
            thread::sleep(IDLE_SLEEP);
        }
    }

    fn keep(&mut self, runner: &mut Runner, asked: &mut Asked) {
        while let Some(event) = runner.engine.take_event() {
            self.note_verdict(event);
        }
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
                    link => Some(HeardFrom::Board(link)),
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

    /// Keeps what the node said of a block or payment handed to it, so the Lab's bug exploiter
    /// reads from a running node, never from the side that sent the trick, whether it was refused
    /// and the rule the node named. Other events the screen already reads from the view.
    fn note_verdict(&self, event: olc_node::Event) {
        use olc_node::Event::{BlockAccepted, BlockRejected, TxAccepted, TxRejected};
        // A block the node mines itself (`Mined`) is not a judgement on a block handed to it, so it
        // is left out: were it kept, a block found while a trick is in flight could be read as the
        // node taking the trick.
        let verdict = match event {
            BlockAccepted { .. } => Some((true, None)),
            BlockRejected { reason } => Some((true, Some(format!("{reason:?}")))),
            TxAccepted => Some((false, None)),
            TxRejected { reason } => Some((false, Some(format!("{reason:?}")))),
            _ => None,
        };
        let Some((block, refused)) = verdict else {
            return;
        };
        if let Ok(mut kept) = self.state.verdicts.lock() {
            kept.0 += 1;
            let number = kept.0;
            if kept.1.len() >= KEPT_VERDICTS {
                kept.1.pop_front();
            }
            kept.1.push_back(super::snapshot::Verdict { number, block, refused });
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
        self.watch = super::chain_view::ChainWatch::default();
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
            let by_hand = self.state.by_hand.lock().map(|by_hand| by_hand.clone()).unwrap_or_default();
            for peer in switch.peers.into_iter().chain(by_hand) {
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
}
