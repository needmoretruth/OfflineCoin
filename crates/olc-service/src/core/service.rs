//! The thread behind the window: it takes commands, keeps the node, the looks around and the node
//! processes going, and publishes a fresh snapshot a few times a second.
use super::coins::{
    bring_old_blocks, coin_folder, format_coins, load_coin, parse_coins, read_coin_file,
    save_coin, write_coin_file,
};
use super::board_action::BoardAction;
use olc_store::coin_file::new_coin;
use super::board_lines::BoardsTold;
use super::commands::{Command, Personal, StartCoin};
use super::found::{link_of, Found};
use super::host::{Host, PayError, Plan};
use super::host_parts::full_rate;
use super::pool_run::{PoolSeat, PoolStart};
use olc_platform::platform;
use super::rows::{
    board_entry, coin_of_board, device_kind, hostname, keeps_of, merge_local, own_entry, reach_of_board,
};
use super::radios::bluetooth_here;
use super::returning::{is_returning, Returning};
use super::snapshot::{BoardView, ExperimentView, Notice, ScanView, Snapshot};
use crate::discovery::{group, Identity, Scanner, Shout};
use crate::fleet::{BoardRow, CapTest, FleetPort};
use crate::local::{NodeSpec, Preset, Supervisor};
use crate::settings::{Paths, Settings, PORT_TRIES};
use olc_board::{Args, Extra};
use olc_core::{CoinConfig, Hash};
use olc_node::Card;
use olc_wallet::WalletError;
use olc_store::NodeSettings;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;


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
        let service = Service::new(launch, settings, snapshot.clone())?;
        thread::spawn(move || service.run(&receiver));
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
    full_rate: u64,
    notice: Option<Notice>,
    notices: Arc<AtomicU64>,
    /// Payments and board lines end on threads of their own and report here.
    late: LateNotices,
    pool: PoolSeat,
    out: Arc<Mutex<Arc<Snapshot>>>,
}

impl Service {
    fn new(
        launch: CoreLaunch,
        settings: Settings,
        out: Arc<Mutex<Arc<Snapshot>>>,
    ) -> Result<Self, String> {
        let paths = launch.paths;
        let coin = current_coin(&paths, &settings);
        bring_old_blocks(&paths, &coin);
        let host = Host::start(
            Plan {
                folder: paths.coin(&coin_folder(&coin)),
                coin,
                wallet_folder: paths.wallet(),
                bind_host: bind_host(&settings),
                port: settings.port,
                port_tries: PORT_TRIES,
                peers: settings.peers.clone(),
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
            },
            launch.fleet.clone(),
        )?;
        if let Some(fleet) = &launch.fleet {
            fleet.attach(host.runner.clone());
        }
        let local = match launch.node_program {
            Some(program) => Supervisor::with_program(paths.clone(), program),
            None => Supervisor::new(paths.clone())?,
        };
        let service = Self {
            paths,
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
            full_rate: full_rate(),
            notice: None,
            notices: Arc::default(),
            late: Arc::default(),
            pool: PoolSeat::default(),
            out,
        };
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
                Ok(command) => self.handle(command),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    self.local.stop_all();
                    return;
                }
            }
            while let Ok(command) = commands.try_recv() {
                self.handle(command);
            }
            self.listen();
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
            Command::Own(Personal::SetLight(on)) => self.change_node(|node| node.light = on),
            Command::Own(Personal::SetBluetooth(on)) if on && !bluetooth_here() => {
                self.say("notice.no_bluetooth", String::new(), false);
            }
            Command::Own(Personal::SetBluetooth(on)) => self.change_node(|node| node.bluetooth = on),
            Command::Own(order) => {
                let (key, detail, good) = self.host.obey(order);
                if !key.is_empty() {
                    self.say(key, detail, good);
                }
            }
            Command::FinishSetup(settings, start) => self.finish_setup(*settings, start),
            Command::Save(settings) => {
                self.settings.take_choices(&settings);
                self.host.set_cap(self.settings.hash_cap);
                self.save();
                self.plan_scan();
            }
            Command::Join { coin, address } => self.join(*coin, address),
            Command::SetMining(on) => {
                self.host.set_mining(on);
                self.settings.mine_on_start = on;
                self.save();
            }
            Command::SetCap(cap) => {
                self.host.set_cap(cap);
                self.settings.hash_cap = cap;
                self.save();
            }
            Command::Pay { to, amount } => self.pay(&to, &amount),
            Command::AddAddress(address) => self.add_address(address),
            Command::Rescan => self.scanner.look_now(),
            Command::SetLinks(rank) => self.set_links(rank),
            Command::OpenDataFolder => {
                let _ = platform().open_folder(&self.paths.root);
            }
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
            Command::StartPool {
                share_difficulty,
                nodes,
                boards,
            } => {
                let specs = self.settings.nodes.clone();
                let start = PoolStart {
                    share_difficulty,
                    specs: &specs,
                    nodes: &nodes,
                    boards: &boards,
                };
                self.pool.start(&self.host, &mut self.local, self.fleet.as_ref(), &start);
            }
            Command::StopPool => self.pool.stop(&mut self.local, self.fleet.as_ref()),
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
        self.settings = settings;
        self.settings.setup_done = true;
        let wanted = self.settings.device.defaults().local_nodes;
        for id in self.settings.nodes.len() as u32..wanted {
            self.settings.nodes.push(Preset::Esp32S3.spec(id));
        }
        match start {
            StartCoin::Default => self.join(CoinConfig::olc(), None),
            StartCoin::New(coin) => self.join(coin, None),
            StartCoin::Join { coin, address } => self.join(coin, address),
        }
        self.host.set_cap(self.settings.hash_cap);
        self.host.set_mining(self.settings.mine_on_start);
        self.save();
        self.plan_scan();
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
                self.host.switch(coin, folder, self.settings.peers.clone(), None);
            }
            Err(error) => self.say("notice.save_failed", error, false),
        }
    }

    /// Moves this app to a coin a person picked. Every node process follows it.
    fn join(&mut self, coin: CoinConfig, address: Option<String>) {
        if self.settings.coin.as_deref() == Some(coin_folder(&coin).as_str()) {
            if let Some(address) = address {
                self.settings.peers.push(address.clone());
                self.save();
                self.host.connect(address);
            }
            return;
        }
        let name = coin.name.clone();
        if self.move_to(coin, address.into_iter().collect(), None) {
            self.say("notice.joined", name, true);
        }
    }

    /// Takes this node and its local nodes to `coin`. A fork names the folder it `leaves`, whose
    /// chain it carries on from.
    fn move_to(&mut self, coin: CoinConfig, peers: Vec<String>, leaves: Option<String>) -> bool {
        let name = coin_folder(&coin);
        if let Err(error) = save_coin(&self.paths, &coin) {
            self.say("notice.save_failed", error, false);
            return false;
        }
        self.settings.coin = Some(name.clone());
        self.settings.peers = peers;
        self.save();
        let keep_from = leaves.as_ref().map(|old| self.paths.coin(old));
        let (folder, peers) = (self.paths.coin(&name), self.settings.peers.clone());
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

    fn pay(&mut self, to: &str, amount: &str) {
        let Some(units) = parse_coins(amount).filter(|units| *units > 0) else {
            self.say("notice.bad_amount", amount.to_string(), false);
            return;
        };
        let result = match self.host.pay(to, units) {
            Ok(_) => ("notice.sent", format_coins(units), true),
            Err(PayError::BadAddress) => ("notice.bad_address", String::new(), false),
            Err(PayError::Wallet(WalletError::NotEnoughCoins)) => {
                ("notice.not_enough", String::new(), false)
            }
            Err(error) => ("notice.pay_failed", format!("{error:?}"), false),
        };
        self.say(result.0, result.1, result.2);
    }

    fn publish(&mut self) {
        let node = self.host.view();
        let identity = node.coin.clone().map(Identity::of);
        let rows = self.fleet.as_ref().map(|fleet| fleet.board_rows());
        let mut rows = rows.unwrap_or_default();
        self.returning.mark(&mut rows);
        let mut entries = self
            .found
            .entries((self.host.address(), node.port.unwrap_or(0)));
        entries.push(own_entry(&node, identity.clone()));
        let said = |row: &BoardRow| {
            let by_link = self.fleet.as_ref().and_then(|fleet| fleet.hello_of(row.seen.id));
            by_link.or_else(|| self.found.board(&row.seen.beacon.address).cloned())
        };
        let hellos: Vec<_> = rows.iter().map(said).collect();
        let links: Vec<_> = hellos.iter().map(|hello| hello.as_ref().and_then(link_of)).collect();
        let answers = hellos.iter().map(|hello| hello.as_ref().map(|hello| Identity::of(hello.coin.clone())));
        let answers: Vec<_> = answers.collect();
        let mut known: Vec<_> = entries.iter().filter_map(|entry| entry.identity.clone()).collect();
        known.extend(answers.iter().flatten().cloned());
        entries.extend(rows.iter().zip(answers).zip(&hellos).map(|((row, said), hello)| {
            board_entry(row, coin_of_board(row.seen.beacon.genesis, said, &known), hello.as_ref())
        }));
        if let Some(fleet) = &self.fleet {
            let auto = self.settings.auto_update;
            let boards = self.settings.auto_boards.iter().filter(|_| auto);
            fleet.keep_updated(boards.copied().collect());
        }
        let specs = self.settings.nodes.clone();
        let local: Vec<_> = specs.iter().map(|spec| self.local.view(spec)).collect();
        merge_local(&mut entries, &local, identity.as_ref());
        let told = self.told.lock().map(|known| known.clone()).unwrap_or_default();
        let boards = rows
            .into_iter()
            .zip(links)
            .zip(hellos.iter().map(|hello| keeps_of(hello.as_ref())))
            .map(|((row, link), keeps)| BoardView {
                keeps,
                returning: is_returning(&row),
                reach: reach_of_board(&row, link),
                state: told.get(&row.seen.id).map(|told| told.state.clone()).unwrap_or_default(),
                answer: told.get(&row.seen.id).and_then(|told| told.answer.clone()),
                seen: row.seen,
                job: row.job,
            })
            .collect();
        let fleet = self.fleet.as_ref();
        let new_ports = fleet.map_or_else(Vec::new, |fleet| {
            let ports = fleet.new_boards();
            ports
                .into_iter()
                .map(|port| {
                    let job = fleet.job(&port);
                    (port, job)
                })
                .collect()
        });
        let (lines, running) = fleet.map_or_else(Default::default, |fleet| fleet.run_log());
        let snapshot = Snapshot {
            groups: group(&entries, identity.as_ref()),
            node,
            boards,
            local,
            new_ports,
            scan: ScanView {
                running: self.scanner.running(),
                tried: self.scanner.tried(),
                subnets: self.scanner.subnets(),
                age_secs: self.scanner.age().map(|age| age.as_secs()),
            },
            experiment: ExperimentView { lines, running },
            pool: self.pool.view(),
            notice: self.notice.clone(),
            usb: self.usb,
            full_rate: self.full_rate,
            folder: self.paths.root.display().to_string(),
        };
        if let Ok(mut out) = self.out.lock() {
            *out = Arc::new(snapshot);
        }
    }
}

/// The start of a hash, for reading.
pub fn short_hex(hash: &Hash) -> String {
    hash[..4].iter().map(|byte| format!("{byte:02x}")).collect()
}
