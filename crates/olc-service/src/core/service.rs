//! The thread behind the window: it takes commands, keeps the node, the looks around and the node
//! processes going, and publishes a fresh snapshot a few times a second.
use super::coins::{
    bring_old_blocks, coin_folder, format_coins, load_coin, parse_coins, save_coin,
};
use super::board_lines::{read_bright, run_line, Brights};
use super::commands::{Command, StartCoin};
use super::found::Found;
use super::host::{full_rate, Host, PayError, Plan};
use super::pool_run::{PoolSeat, PoolStart};
use olc_platform::platform;
use super::rows::{
    board_entry, coin_of_board, device_kind, hostname, merge_local, own_entry, reach_of_board,
};
use super::returning::{is_returning, Returning};
use super::snapshot::{BoardView, ExperimentView, Notice, ScanView, Snapshot};
use crate::discovery::{group, Identity, Scanner, Shout};
use crate::fleet::{BoardRow, CapTest, FleetPort};
use crate::local::{NodeSpec, Preset, Supervisor};
use crate::settings::{Paths, Settings, PORT_TRIES};
use olc_board::{Args, Extra};
use olc_control::BoardId;
use olc_core::{CoinConfig, Hash};
use olc_node::Card;
use olc_wallet::WalletError;
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

mod listen;

struct Service {
    paths: Paths,
    settings: Settings,
    host: Host,
    fleet: Option<Arc<dyn FleetPort>>,
    usb: bool,
    brights: Brights,
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
            brights: Brights::default(),
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
            Command::OpenDataFolder => {
                let _ = platform().open_folder(&self.paths.root);
            }
            Command::BoardLine { id, line } => self.board_line(id, line),
            Command::UpdateBoard(id) => {
                if let Some(fleet) = &self.fleet {
                    fleet.start_update(id);
                }
            }
            Command::MoveBoard(id) => {
                let coin = current_coin(&self.paths, &self.settings);
                let bytes = postcard::to_allocvec(&coin).unwrap_or_default();
                let text: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
                self.board_line(id, format!("coin {text}"));
                self.board_line(id, "coin confirm".into());
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
            Command::ReadBright(board) => self.read_bright(board),
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

    /// Moves this app to a coin a person picked. Every node process follows it.
    fn join(&mut self, coin: CoinConfig, address: Option<String>) {
        let name = coin_folder(&coin);
        if self.settings.coin.as_deref() == Some(name.as_str()) {
            if let Some(address) = address {
                self.settings.peers.push(address.clone());
                self.save();
                self.host.connect(address);
            }
            return;
        }
        if let Err(error) = save_coin(&self.paths, &coin) {
            self.say("notice.save_failed", error, false);
            return;
        }
        self.settings.coin = Some(name.clone());
        self.settings.peers = address.into_iter().collect();
        self.save();
        self.host.switch(
            coin.clone(),
            self.paths.coin(&name),
            self.settings.peers.clone(),
        );
        self.local.stop_all();
        // Their ports stay taken until the old processes have gone.
        thread::sleep(Duration::from_millis(100));
        self.start_nodes();
        self.say("notice.joined", coin.name, true);
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

    fn add_address(&mut self, address: String) {
        let address = address.trim().to_string();
        if address.is_empty() {
            return;
        }
        let full = match address.rsplit_once(':') {
            Some((_, port)) if port.parse::<u16>().is_ok() => address,
            _ => format!("{address}:{}", crate::settings::NODE_PORT),
        };
        if !self.settings.added.contains(&full) {
            self.settings.added.push(full.clone());
        }
        self.found.added(&full);
        self.save();
        self.plan_scan();
        self.scanner.look_now();
    }

    fn read_bright(&mut self, id: BoardId) {
        if let Some(fleet) = self.fleet.clone() {
            read_bright(fleet, self.brights.clone(), id);
        }
    }

    fn board_line(&mut self, id: BoardId, line: String) {
        self.returning.note_line(id, &line);
        if let Some(fleet) = self.fleet.clone() {
            run_line(fleet, (self.late.clone(), self.brights.clone()), id, line);
        }
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
            let hello = by_link.or_else(|| self.found.board(&row.seen.beacon.address).cloned());
            hello.map(|hello| Identity::of(hello.coin))
        };
        let answers: Vec<_> = rows.iter().map(said).collect();
        let mut known: Vec<_> = entries.iter().filter_map(|entry| entry.identity.clone()).collect();
        known.extend(answers.iter().flatten().cloned());
        entries.extend(rows.iter().zip(answers).map(|(row, said)| {
            board_entry(row, coin_of_board(row.seen.beacon.genesis, said, &known))
        }));
        if let Some(fleet) = &self.fleet {
            let auto = self.settings.auto_update;
            let boards = self.settings.auto_boards.iter().filter(|_| auto);
            fleet.keep_updated(boards.copied().collect());
        }
        let specs = self.settings.nodes.clone();
        let local: Vec<_> = specs.iter().map(|spec| self.local.view(spec)).collect();
        merge_local(&mut entries, &local, identity.as_ref());
        let brights = self.brights.lock().map(|known| known.clone()).unwrap_or_default();
        let boards = rows
            .into_iter()
            .map(|row| BoardView {
                returning: is_returning(&row),
                reach: reach_of_board(&row),
                bright: brights.get(&row.seen.id).copied(),
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
