//! The thread behind the window: it takes commands, keeps the node, the looks around and the node
//! processes going, and publishes a fresh snapshot a few times a second.
use super::coins::{
    bring_old_blocks, coin_folder, leave_retired_olc, load_coin, read_coin_file, save_coin, write_coin_file,
};
use super::board_action::BoardAction;
use super::swap_book::Swaps;
use olc_store::coin_file::new_coin;
use super::board_lines::BoardsTold;
use super::commands::{Command, Personal, StartCoin};
use super::found::Found;
use super::host::{Host, Plan};
use super::host_parts::full_rate;
use super::pool_run::PoolSeat;
use olc_platform::platform;
use super::rows::{device_kind, hostname};
use super::radios::bluetooth_here;
use super::returning::Returning;
use super::snapshot::{Notice, Snapshot};
use crate::discovery::{Scanner, Shout};
use crate::fleet::{CapTest, FleetPort};
use crate::local::{NodeSpec, Preset, Supervisor};
use crate::settings::{standard_threads, Paths, Settings, PORT_TRIES};
use olc_board::{Args, Extra};
use olc_core::{CoinConfig, Hash};
use olc_node::Card;
use olc_store::NodeSettings;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const PUBLISH_EVERY: Duration = Duration::from_millis(400);

pub struct CoreLaunch {
    pub paths: Paths,
    pub fleet: Option<Arc<dyn FleetPort>>,
    /// False when this run leaves USB alone.
    pub usb: bool,
    /// The program node processes run. By default the one running now.
    pub node_program: Option<PathBuf>,
}

/// The window's handle on all of it.
#[derive(Clone)]
pub struct Core {
    snapshot: Arc<Mutex<Arc<Snapshot>>>,
    commands: Sender<Command>,
}

impl Core {
    pub fn start(launch: CoreLaunch, settings: Settings) -> Result<Self, String> {
        let (commands, receiver) = channel();
        let snapshot = Arc::new(Mutex::new(Arc::new(Snapshot::default())));
        let wallet = launch.paths.wallet();
        let service = Service::new(launch, settings, snapshot.clone())?;
        thread::spawn(move || service.run(&receiver));
        // The `olc` program's words reach this core while anyone holds it: the service's own
        // thread holds one share of the snapshot and a word being answered another.
        let (held, sender) = (Arc::downgrade(&snapshot), commands.clone());
        super::door::serve_core(wallet, move || {
            let snapshot = held.upgrade().filter(|strong| Arc::strong_count(strong) > 2)?;
            Some(Self { snapshot, commands: sender.clone() })
        });
        Ok(Self { snapshot, commands })
    }

    pub fn snapshot(&self) -> Arc<Snapshot> {
        self.snapshot
            .lock()
            .map(|snapshot| snapshot.clone())
            .unwrap_or_default()
    }

    pub fn send(&self, command: Command) {
        let _ = self.commands.send(command);
    }

    /// Waits until what a person asked for is done, so closing the window while a chain loads
    /// takes nothing with it.
    pub fn finish(&self) {
        let (done, wait) = channel();
        self.send(Command::Finish(done));
        let _ = wait.recv();
    }
}

fn current_coin(paths: &Paths, settings: &Settings) -> CoinConfig {
    settings
        .coin
        .as_deref()
        .and_then(|name| load_coin(paths, name))
        .unwrap_or_else(CoinConfig::olc)
}

fn bind_host(settings: &Settings) -> String {
    match settings.listen_lan {
        true => "0.0.0.0".into(),
        false => "127.0.0.1".into(),
    }
}

/// Notices from threads the service started: key, detail, and whether it went well.
pub type LateNotices = Arc<Mutex<Vec<(&'static str, String, bool)>>>;

mod forks;
mod locks;
mod listen;
mod orders;
mod view;
mod channels;
mod swaps;
mod making;

struct Service {
    paths: Paths,
    settings: Settings,
    host: Host,
    fleet: Option<Arc<dyn FleetPort>>,
    usb: bool,
    told: BoardsTold,
    returning: Returning,
    scanner: Scanner,
    shout: Shout,
    found: Found,
    local: Supervisor,
    /// Hashes a second on all mining threads together, measured at the start.
    full_rate: u64,
    /// What one of those threads did beside the others.
    thread_rate: u64,
    /// The settings as the app last sent them, so a save takes only what it changed.
    page: Settings,
    notice: Option<Notice>,
    notices: Arc<AtomicU64>,
    asked_boards: Option<Instant>,
    /// Orders that wait for a chain on its way in, oldest first.
    held: VecDeque<Command>,
    /// Payments and board lines end on threads of their own and report here.
    late: LateNotices,
    pool: PoolSeat,
    /// Blocks being made now, as `generate N` asked.
    maker: super::making::Maker,
    swaps: Swaps,
    /// This computer's speed over the last ten minutes, for the speed line.
    trail: super::chain_race::Trail,
    out: Arc<Mutex<Arc<Snapshot>>>,
}

impl Service {
    fn new(
        launch: CoreLaunch,
        mut settings: Settings,
        out: Arc<Mutex<Arc<Snapshot>>>,
    ) -> Result<Self, String> {
        let paths = launch.paths;
        let left_old_olc = leave_retired_olc(&paths, &mut settings).is_some();
        let coin = current_coin(&paths, &settings);
        bring_old_blocks(&paths, &coin);
        // Measured before anything mines, on as many cores as will mine.
        let threads = settings.threads(standard_threads());
        let measured = full_rate(threads);
        let host = Host::start(
            Plan {
                folder: paths.coin(&coin_folder(&coin)),
                coin,
                wallet_folder: paths.wallet(),
                bind_host: bind_host(&settings),
                port: settings.port,
                port_tries: PORT_TRIES,
                peers: settings.to_dial(Some(settings.port)),
                cap: settings.hash_cap,
                // Nothing mines before a person has chosen a coin.
                mining: settings.setup_done && settings.mine_on_start,
                card: Card {
                    name: hostname(),
                    device: device_kind(),
                    build: env!("CARGO_PKG_VERSION").into(),
                    ..Card::default()
                },
                storage: None,
                shares_wallet: false,
            },
            launch.fleet.clone(),
        )?;
        host.set_threads(threads);
        if let Some(fleet) = &launch.fleet {
            fleet.attach(host.runner.clone());
        }
        let local = match launch.node_program {
            Some(program) => Supervisor::with_program(paths.clone(), program),
            None => Supervisor::new(paths.clone())?,
        };
        let mut service = Self {
            swaps: Swaps::load(paths.wallet()),
            trail: super::chain_race::Trail::default(),
            paths,
            page: settings.clone(),
            settings,
            host,
            fleet: launch.fleet,
            usb: launch.usb,
            told: BoardsTold::default(),
            returning: Returning::default(),
            scanner: Scanner::start(),
            shout: Shout::start(),
            found: Found::default(),
            local,
            full_rate: measured,
            thread_rate: measured / threads.max(1) as u64,
            notice: None,
            notices: Arc::default(),
            asked_boards: None,
            held: VecDeque::new(),
            late: Arc::default(),
            pool: PoolSeat::default(),
            maker: Default::default(),
            out,
        };
        if left_old_olc {
            service.say("notice.olc_restarted", String::new(), true);
        }
        Ok(service)
    }

    fn run(mut self, commands: &Receiver<Command>) {
        for address in self.settings.added.clone() {
            self.found.added(&address);
        }
        self.plan_scan();
        self.scanner.look_now();
        if self.settings.setup_done {
            self.start_nodes();
        }
        loop {
            match commands.recv_timeout(PUBLISH_EVERY) {
                Ok(command) => self.take(command),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    self.local.stop_all();
                    return;
                }
            }
            while let Ok(command) = commands.try_recv() {
                self.take(command);
            }
            self.let_go();
            self.listen();
            self.step_making();
            self.step_swaps();
            self.follow_payment();
            self.publish();
        }
    }

    fn say(&mut self, key: &'static str, detail: String, good: bool) {
        self.notice = Some(Notice {
            number: self.notices.fetch_add(1, Ordering::Relaxed) + 1,
            key,
            detail,
            good,
        });
    }

    fn save(&mut self) {
        if let Err(error) = self.settings.save(&self.paths.root) {
            self.say("notice.save_failed", error, false);
        }
    }

    fn handle(&mut self, command: Command) {
        match command {
            Command::Finish(_) => {}
            Command::Own(Personal::SetLight(on)) => self.change_node(|node| node.light = on),
            Command::Own(Personal::SetBluetooth(on)) if on && !bluetooth_here() => {
                self.say("notice.no_bluetooth", String::new(), false);
            }
            Command::Own(Personal::SetBluetooth(on)) => self.change_node(|node| node.bluetooth = on),
            // The console's `mining on|off`: mine now, and from every start of the app.
            Command::Own(Personal::SetMining(on)) => {
                self.host.set_mining(on);
                self.settings.mine_on_start = on;
                self.save();
                self.say("notice.saved", String::new(), true);
            }
            // No node runs before the first run's questions are done; words go to the wallet's folder.
            Command::Own(Personal::Wallet(order)) if self.host.ready().is_err() => self.wallet_in_folder(order),
            Command::Own(order) => {
                let (key, detail, good) = self.host.obey(order);
                if !key.is_empty() {
                    self.say(key, detail, good);
                }
            }
            Command::FinishSetup(settings, start) => self.finish_setup(*settings, start),
            Command::Save(settings) => self.save_settings(&settings),
            Command::Join { coin, address } => self.join(*coin, address),
            // Start and Stop act now. Whether mining starts with the app is a setting of its own.
            Command::SetMining(on) => self.host.set_mining(on),
            Command::SetCap(cap) => {
                self.host.set_cap(cap);
                self.settings.hash_cap = cap;
                self.save();
            }
            Command::Pay { to, amount } => self.pay(&to, &amount),
            Command::AddAddress(address) => self.add_address(address),
            Command::Rescan => self.scanner.look_now(),
            Command::SetLinks(rank) => self.set_links(rank),
            Command::OpenDataFolder => drop(platform().open_folder(&self.paths.root)),
            Command::OpenSettingsFile => drop(platform().open_folder(&self.paths.settings_file())),
            Command::ResetNodePreferences => self.reset_node(),
            Command::Board { id, action } => self.board_action(id, action),
            Command::NewCoinFile { choices, path } => {
                let made = new_coin(&choices).map_err(|bad| bad.to_string());
                match made.and_then(|coin| write_coin_file(&coin, &path)) {
                    Ok(()) => self.say("notice.coin_saved", path, true),
                    Err(detail) => self.say("notice.coin_file", detail, false),
                }
            }
            Command::Fork { at, changes } => self.fork(at, &changes),
            Command::ForkBack => self.fork_back(),
            Command::Lock(order) => self.lock_order(order),
            Command::Channel(order) => self.channel_order(order),
            Command::Swap(order) => self.swap_order(order),
            Command::JoinCoinFile(path) => match read_coin_file(&path) {
                Ok(coin) => self.join(coin, None),
                Err(detail) => self.say("notice.coin_file", detail, false),
            },
            Command::SaveCoinFile(path) => {
                let coin = current_coin(&self.paths, &self.settings);
                match write_coin_file(&coin, &path) {
                    Ok(()) => self.say("notice.coin_saved", path, true),
                    Err(detail) => self.say("notice.coin_file", detail, false),
                }
            }
            Command::UpdateBoard(id) => {
                if let Some(fleet) = &self.fleet {
                    fleet.start_update(id);
                }
            }
            Command::MoveBoard(id) => {
                let coin = current_coin(&self.paths, &self.settings);
                let text: String = coin.to_bytes().iter().map(|byte| format!("{byte:02x}")).collect();
                self.board_action(id, BoardAction::MoveToCoin(text));
                self.board_action(id, BoardAction::ConfirmCoin);
            }
            Command::InstallBoard { path, name } => {
                let started = self
                    .fleet
                    .as_ref()
                    .is_some_and(|fleet| fleet.start_install(&path, &name));
                if !started {
                    self.say("notice.install_refused", name, false);
                }
            }
            Command::StartExperiment { boards, args } => {
                let started = self
                    .fleet
                    .as_ref()
                    .map(|fleet| fleet.start_extra(&olc_lab::Lab, &boards, args));
                if !matches!(started, Some(Ok(()))) {
                    self.say("notice.experiment_busy", String::new(), false);
                }
            }
            Command::ReadBoardState(board) => self.read_state(board),
            Command::StartCapTest(board) => {
                let words = [CapTest.command().to_string()];
                let args = Args::parse(words.into_iter());
                let started = self
                    .fleet
                    .as_ref()
                    .map(|fleet| fleet.start_extra(&CapTest, &[board], args));
                if !matches!(started, Some(Ok(()))) {
                    self.say("notice.experiment_busy", String::new(), false);
                }
            }
            Command::StopExperiment => {
                if let Some(fleet) = &self.fleet {
                    fleet.stop_extra();
                }
            }
            Command::AddNode(spec) => {
                self.settings.nodes.push(*spec.clone());
                self.save();
                self.start_node(&spec);
            }
            Command::RemoveNode(id) => {
                self.local.stop(id);
                self.settings.nodes.retain(|spec| spec.id != id);
                self.save();
            }
            Command::StartNode(id) => {
                if let Some(spec) = self.spec(id) {
                    self.start_node(&spec);
                }
            }
            Command::StopNode(id) => self.local.stop(id),
            Command::NodeCap { id, cap } => {
                if let Some(spec) = self.settings.nodes.iter_mut().find(|spec| spec.id == id) {
                    spec.hash_cap = cap;
                }
                self.save();
                let _ = self.local.send(id, &format!("limit {cap}"));
            }
            Command::NodeMining { id, on } => {
                if let Some(spec) = self.settings.nodes.iter_mut().find(|spec| spec.id == id) {
                    spec.mining = on;
                }
                self.save();
                let word = if on { "on" } else { "off" };
                let _ = self.local.send(id, &format!("mine {word}"));
            }
            Command::NodeMiner { id, to } => {
                let target = if to.trim().is_empty() { "own" } else { to.trim() };
                let _ = self.local.send(id, &format!("mineto {target}"));
            }
            Command::Pool(order) => self.pool_order(order),
            Command::Chain(order) => self.chain_order(order),
            Command::NodePay { id, to, amount } => {
                let _ = self.local.send(id, &format!("pay {} {}", to.trim(), amount.trim()));
            }
        }
    }

    fn spec(&self, id: u32) -> Option<NodeSpec> {
        self.settings.nodes.iter().find(|spec| spec.id == id).cloned()
    }

    fn peers_of(&self, spec: &NodeSpec) -> Vec<String> {
        if !spec.joined {
            return Vec::new();
        }
        let earlier = self
            .settings
            .nodes
            .iter()
            .filter(|other| other.joined && other.id < spec.id)
            .map(crate::local::address_of);
        let own = self.host.port.map(|port| format!("127.0.0.1:{port}"));
        earlier.chain(own).collect()
    }

    fn start_node(&mut self, spec: &NodeSpec) {
        let coin = current_coin(&self.paths, &self.settings);
        let peers = self.peers_of(spec);
        let started = save_coin(&self.paths, &coin)
            .and_then(|file| self.local.start(spec, &file, &peers));
        if let Err(error) = started {
            self.say("notice.node_failed", error, false);
        }
    }

    fn start_nodes(&mut self) {
        for spec in self.settings.nodes.clone() {
            self.start_node(&spec);
        }
    }

    fn finish_setup(&mut self, settings: Settings, start: StartCoin) {
        self.page = settings.clone();
        self.settings = settings;
        self.settings.setup_done = true;
        self.add_wanted_nodes();
        match start {
            StartCoin::Default => self.join(CoinConfig::olc(), None),
            StartCoin::New(coin) => self.join(coin, None),
            StartCoin::Join { coin, address } => self.join(coin, address),
        }
        self.host.set_cap(self.settings.hash_cap);
        self.set_threads();
        self.host.set_mining(self.settings.mine_on_start);
        self.save();
        self.plan_scan();
    }

    /// Miners the device choice asks for that are not there yet, sized for this computer.
    pub(super) fn add_wanted_nodes(&mut self) -> Vec<NodeSpec> {
        let wanted = self.settings.device.defaults().local_nodes;
        let have = self.settings.nodes.len() as u32;
        let next = self.settings.nodes.iter().map(|spec| spec.id + 1).max().unwrap_or(0);
        let ids = next..next + wanted.saturating_sub(have);
        let added: Vec<NodeSpec> = ids.map(|id| Preset::default().spec(id)).collect();
        self.settings.nodes.extend(added.iter().cloned());
        added
    }

    /// Mining threads as the settings say, and the full rate that goes with them.
    pub(super) fn set_threads(&mut self) {
        let threads = self.settings.threads(standard_threads());
        self.host.set_threads(threads);
        self.full_rate = self.thread_rate * threads as u64;
    }

    /// The node comes back as the other kind on the same coin and the same folder.
    /// Changes how this node runs. It takes the change by starting over on the same coin.
    fn change_node(&mut self, change: impl FnOnce(&mut NodeSettings)) {
        let Some(name) = self.settings.coin.clone() else {
            return;
        };
        let folder = self.paths.coin(&name);
        let mut settings = NodeSettings::load(&folder);
        change(&mut settings);
        match settings.save(&folder) {
            Ok(()) => {
                let coin = current_coin(&self.paths, &self.settings);
                self.host.switch(coin, folder, self.settings.to_dial(self.host.port), None);
            }
            Err(error) => self.say("notice.save_failed", error, false),
        }
    }

    /// Moves this app to a coin a person picked. Every node process follows it.
    /// Takes this node and its local nodes to `coin`. A fork names the folder it `leaves`, whose
    /// chain it carries on from.
    pub(super) fn move_to(&mut self, coin: CoinConfig, peers: Vec<String>, leaves: Option<String>) -> bool {
        if coin.is_retired_olc() {
            self.say("notice.olc_retired", String::new(), false);
            return false;
        }
        let name = coin_folder(&coin);
        if let Err(error) = save_coin(&self.paths, &coin) {
            self.say("notice.save_failed", error, false);
            return false;
        }
        self.settings.coin = Some(name.clone());
        self.settings.peers = peers;
        self.save();
        let keep_from = leaves.as_ref().map(|old| self.paths.coin(old));
        let (folder, peers) = (self.paths.coin(&name), self.settings.to_dial(self.host.port));
        self.host.switch(coin, folder, peers, keep_from);
        self.local.stop_all();
        // Their ports stay taken until the old processes have gone.
        thread::sleep(Duration::from_millis(100));
        if let Some(old) = &leaves {
            self.carry_local_chains(old, &name);
        }
        self.start_nodes();
        true
    }
}

/// The start of a hash, for reading.
pub fn short_hex(hash: &Hash) -> String {
    hash[..4].iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod kept_tests;
