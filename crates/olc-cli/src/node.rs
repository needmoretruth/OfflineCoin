//! Runs a node: the links, the mining loop and the browser screen.
use crate::app::App;
use crate::{coinfile, web, window};
use olc_board::{autoupdate, carried, Args};
use olc_core::{CoinConfig, Plugins};
use olc_link_serial::SerialLink;
use olc_link_tcp::TcpLink;
use olc_node::{Card, Engine, Event, Medium, Rank, Runner};
use olc_pow::ProofOfWork;
use olc_service::core::radios;
use olc_store::{load_headers, load_wallet, open_chain, save_headers, NodeSettings};
use olc_wallet::parse_address;
use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const MINING_BATCH: u32 = 5_000;
/// The cap of a computer whose owner never chose one.
const RELAXED_LIMIT: u64 = 20_000;
const LIMIT_FILE: &str = "hash-limit";
const IDLE_SLEEP: Duration = Duration::from_millis(5);
const MAX_LOG: usize = 40;
const HEADERS_SAVED_EVERY: Duration = Duration::from_secs(30);

/// Shared between the mining loop and the browser screen.
pub struct Shared {
    pub runner: Arc<Mutex<Runner>>,
    pub mining: AtomicBool,
    pub hashrate: AtomicU64,
    /// Hashes per second, 0 for full.
    pub hash_limit: AtomicU64,
    /// What this computer does with no cap, for the page's half-way choice.
    pub full_rate: u64,
    pub directory: PathBuf,
    pub log: Mutex<VecDeque<(u64, Event)>>,
    pub siblings: Vec<u16>,
    /// What the app adds to a plain node: the boards it runs.
    pub app: Option<Arc<App>>,
}

pub fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |time| time.as_millis() as u64)
}

pub fn run(args: &Args) -> Result<(), String> {
    let directory = PathBuf::from(args.value("data").unwrap_or(".olc/node"));
    start(args, directory, None)
}

/// A node with the boards around it on its page. It holds the control key, so it never opens to
/// the network.
pub fn run_app(args: &Args) -> Result<(), String> {
    if args.has("web-public") {
        return Err("The app holds the control key, so it stays on this computer.".into());
    }
    let folder = olc_board::control_key::config_folder()?;
    if window::open_existing(&folder) {
        return Ok(());
    }
    let _only_copy = window::claim(&folder)?;
    let directory = match args.value("data") {
        Some(data) => PathBuf::from(data),
        None => folder.join("node"),
    };
    start(args, directory, Some(App::new(args)?))
}

fn start(args: &Args, directory: PathBuf, app: Option<Arc<App>>) -> Result<(), String> {
    let coin = coinfile::load(args.value("coin"))?;
    // The app takes whatever is free and stays on this computer, so nobody picks a port for it.
    let (default_port, default_web, host) = match (&app, args.has("web-public")) {
        (Some(_), _) => (0, 0, "127.0.0.1"),
        (None, true) => (7000, 8000, "0.0.0.0"),
        (None, false) => (7000, 8000, "127.0.0.1"),
    };
    let port: u16 = args.number("port", default_port)?;
    let web_port: u16 = args.number("web", default_web)?;
    let hash_limit = chosen_limit(args, &directory)?;

    let engine = open_engine(&directory, coin)?;
    let mut runner = Runner::new(engine);
    runner.set_id(fresh_id()?);
    if let Some(rank) = chosen_rank(args, &directory)? {
        runner.set_rank(rank);
    }
    let tcp = TcpLink::new();
    let listen_on = if app.is_some() { host } else { "0.0.0.0" };
    tcp.listen(&format!("{listen_on}:{port}"))
        .map_err(|error| format!("port {port}: {error}"))?;
    runner.card = Card {
        device: "computer".into(),
        name: args.value("name").unwrap_or_default().to_string(),
        build: env!("CARGO_PKG_VERSION").into(),
        port,
        known: args.values("connect"),
        ..Card::default()
    };
    let tcp = runner.add_link(Box::new(tcp));
    for address in args.values("connect") {
        runner.add_address(tcp, address);
    }
    add_radios(args, &mut runner, &directory)?;
    if app.is_none() {
        for path in args.values("serial") {
            let link =
                SerialLink::open(&path, |_| {}).map_err(|error| format!("{path}: {error}"))?;
            runner.add_link(Box::new(link));
        }
    }

    let siblings = args
        .value("siblings")
        .map(|list| {
            list.split(',')
                .filter_map(|port| port.parse().ok())
                .collect()
        })
        .unwrap_or_default();
    let shared = Arc::new(Shared {
        runner: Arc::new(Mutex::new(runner)),
        mining: AtomicBool::new(args.has("mine") || NodeSettings::load(&directory).mining),
        hashrate: AtomicU64::new(0),
        hash_limit: AtomicU64::new(hash_limit),
        full_rate: measure_full_rate(),
        directory: directory.clone(),
        log: Mutex::new(VecDeque::new()),
        siblings,
        app,
    });
    if let Some(app) = &shared.app {
        app.watch_ports(shared.runner.clone());
        if let (Some(build), false) = (carried::build(), args.has("firmware")) {
            autoupdate::start((**app).clone(), build);
        }
    }
    let web_port = web::serve(shared.clone(), format!("{host}:{web_port}"))?;
    match &shared.app {
        Some(app) => {
            app.web_port.store(web_port, Ordering::Relaxed);
            let folder = olc_board::control_key::config_folder()?;
            window::open_and_watch(web_port, folder, app.clone())?;
        }
        None => println!("http://127.0.0.1:{web_port}"),
    }
    main_loop(&shared);
    Ok(())
}

/// Opens the chain and wallet in `directory` and applies what the owner chose for this node.
fn open_engine(directory: &Path, coin: CoinConfig) -> Result<Engine, String> {
    let wallet = load_wallet(directory)?;
    let plugins = Plugins {
        consensus: Box::new(ProofOfWork),
        languages: olc_lang::all(),
    };
    let chain = open_chain(directory, coin, plugins, None)?;
    let mut engine = Engine::on_chain(chain, wallet);
    let settings = NodeSettings::load(directory);
    engine.set_miner(settings.miner.as_deref().and_then(parse_address));
    engine.set_note(settings.note.as_bytes());
    engine.set_merging(!settings.no_merge);
    settings.watched.iter().filter_map(|text| parse_address(text)).for_each(|a| engine.watch(a));
    if settings.light {
        engine.become_light(usize::MAX, &load_headers(directory));
    }
    Ok(engine)
}

/// Radios the settings or this run ask for.
fn add_radios(args: &Args, runner: &mut Runner, directory: &Path) -> Result<(), String> {
    let mut settings = NodeSettings::load(directory);
    settings.bluetooth |= args.has("ble");
    if let Some(card) = args.value("espnow") {
        settings.esp_now_card = card.to_string();
    }
    let problems = radios::Running::default().bring_up(runner, &settings);
    problems.first().map_or(Ok(()), |problem| Err(problem.clone()))
}

/// What this run goes by on the links. Nothing about it needs to last.
fn fresh_id() -> Result<u64, String> {
    let mut bytes = [0u8; 8];
    getrandom::fill(&mut bytes).map_err(|error| error.to_string())?;
    Ok(u64::from_le_bytes(bytes))
}

/// The order of links: this run's flag, saved if given, then the saved choice.
fn chosen_rank(args: &Args, directory: &Path) -> Result<Option<Rank>, String> {
    let mut settings = NodeSettings::load(directory);
    if let Some(text) = args.value("links") {
        let rank = Rank::parse(text).ok_or("--links needs wired, espnow, bluetooth and wifi, each once")?;
        settings.links = rank.text();
        settings.save(directory)?;
    }
    Ok(Rank::parse(&settings.links))
}

/// The cap to start with: this run's flag, then the saved choice, then the relaxed default.
fn chosen_limit(args: &Args, directory: &Path) -> Result<u64, String> {
    if args.value("hash-limit").is_some() {
        let limit = args.number("hash-limit", 0)?;
        save_limit(directory, Some(limit));
        return Ok(limit);
    }
    let saved = fs::read_to_string(directory.join(LIMIT_FILE)).ok();
    Ok(saved
        .and_then(|text| text.trim().parse().ok())
        .unwrap_or(RELAXED_LIMIT))
}

/// `None` forgets the choice, which goes back to the default.
fn save_limit(directory: &Path, choice: Option<u64>) {
    let file = directory.join(LIMIT_FILE);
    let _ = match choice {
        Some(limit) => fs::write(file, limit.to_string()),
        None => fs::remove_file(file),
    };
}

pub fn set_limit(shared: &Shared, choice: Option<u64>) {
    save_limit(&shared.directory, choice);
    shared
        .hash_limit
        .store(choice.unwrap_or(RELAXED_LIMIT), Ordering::Relaxed);
}

fn measure_full_rate() -> u64 {
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

fn main_loop(shared: &Shared) {
    let mut window = Instant::now();
    let mut hashes = 0u64;
    let mut saved = Instant::now();
    loop {
        let hash_limit = shared.hash_limit.load(Ordering::Relaxed);
        // A batch bigger than the cap allows in a window would overrun it.
        let batch = u32::try_from(hash_limit / 100)
            .ok()
            .filter(|share| *share != 0 || hash_limit != 0)
            .map_or(MINING_BATCH, |share| share.clamp(1, MINING_BATCH));
        let elapsed_ms = window.elapsed().as_millis() as u64;
        let allowed = hash_limit == 0 || hashes < hash_limit * elapsed_ms / 1_000;
        let mining = shared.mining.load(Ordering::Relaxed) && allowed;
        if let Ok(mut runner) = shared.runner.lock() {
            let now = unix_ms();
            runner.step(now);
            if mining {
                runner.engine.mine(batch, now);
                hashes += u64::from(batch);
            }
            if let Some(app) = &shared.app {
                app.pump(&mut runner);
            }
            record_events(shared, &mut runner.engine);
            print_moves(&mut runner);
        }
        if elapsed_ms >= 1_000 {
            shared
                .hashrate
                .store(hashes * 1_000 / elapsed_ms, Ordering::Relaxed);
            if let Ok(mut runner) = shared.runner.lock() {
                runner.card.mining = shared.mining.load(Ordering::Relaxed);
                runner.card.hashrate = hashes * 1_000 / elapsed_ms;
            }
            hashes = 0;
            window = Instant::now();
        }
        if saved.elapsed() >= HEADERS_SAVED_EVERY {
            saved = Instant::now();
            save_light(shared);
        }
        if mining {
            // Lets the browser screen take the lock between batches.
            thread::yield_now();
        } else {
            thread::sleep(IDLE_SLEEP);
        }
    }
}

fn save_light(shared: &Shared) {
    let Ok(runner) = shared.runner.lock() else {
        return;
    };
    let light = runner.engine.light();
    if let Some(headers) = light.and_then(|light| light.headers().saved()) {
        // The next save tries again, and peers can give back whatever is lost.
        let _ = save_headers(&shared.directory, headers);
    }
}

/// Traffic moving to another link is rare and worth a line in any log.
fn print_moves(runner: &mut Runner) {
    while let Some(moved) = runner.take_move() {
        let name = |link: Option<usize>| {
            let medium = link.and_then(|link| runner.medium_of(link));
            medium.map_or("nothing", Medium::name)
        };
        println!("link {:x} {} -> {}", moved.peer, name(moved.from), name(moved.to));
    }
}

fn record_events(shared: &Shared, engine: &mut Engine) {
    while let Some(event) = engine.take_event() {
        if let Ok(mut log) = shared.log.lock() {
            log.push_back((unix_ms(), event));
            if log.len() > MAX_LOG {
                log.pop_front();
            }
        }
    }
}
