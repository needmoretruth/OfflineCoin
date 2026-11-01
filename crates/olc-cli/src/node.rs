//! Runs a node: the links, the mining loop and the browser screen.
use crate::app::App;
use crate::store::{load_or_create_secret, Store};
use crate::{coinfile, web, window};
use olc_board::{autoupdate, carried, Args};
use olc_core::Plugins;
use olc_link_serial::SerialLink;
use olc_link_tcp::TcpLink;
use olc_node::{Engine, Event, Runner};
use olc_pow::ProofOfWork;
use olc_wallet::Wallet;
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

    let secret = load_or_create_secret(&directory)?;
    let wallet = Wallet::from_secret(secret).map_err(|_| "The key file holds a bad key")?;
    let plugins = Plugins {
        consensus: Box::new(ProofOfWork),
        languages: olc_lang::all(),
    };
    let mut engine = Engine::new(coin, plugins, wallet);
    let (mut store, saved) = Store::open(&directory)?;
    engine.begin_restoring();
    for block in saved {
        engine.restore_block(block);
    }
    engine.finish_restoring(|_, _| {});

    let mut runner = Runner::new(engine);
    let tcp = TcpLink::new();
    let listen_on = if app.is_some() { host } else { "0.0.0.0" };
    tcp.listen(&format!("{listen_on}:{port}"))
        .map_err(|error| format!("port {port}: {error}"))?;
    for address in args.values("connect") {
        tcp.connect(address);
    }
    runner.add_link(Box::new(tcp));
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
        mining: AtomicBool::new(args.has("mine")),
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
    main_loop(&shared, &mut store);
    Ok(())
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

fn main_loop(shared: &Shared, store: &mut Store) {
    let mut window = Instant::now();
    let mut hashes = 0u64;
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
            while let Some(block) = runner.engine.take_fresh_block() {
                store.append(&block);
            }
        }
        if elapsed_ms >= 1_000 {
            shared
                .hashrate
                .store(hashes * 1_000 / elapsed_ms, Ordering::Relaxed);
            hashes = 0;
            window = Instant::now();
        }
        if mining {
            // Lets the browser screen take the lock between batches.
            thread::yield_now();
        } else {
            thread::sleep(IDLE_SLEEP);
        }
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
