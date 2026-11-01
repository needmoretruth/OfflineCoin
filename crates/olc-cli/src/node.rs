//! Runs a node: the links, the mining loop and the browser screen.
use crate::app::App;
use crate::store::{load_or_create_secret, Store};
use crate::{coinfile, web};
use olc_board::Args;
use olc_core::Plugins;
use olc_link_serial::SerialLink;
use olc_link_tcp::TcpLink;
use olc_node::{Engine, Event, Runner};
use olc_pow::ProofOfWork;
use olc_wallet::Wallet;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const MINING_BATCH: u32 = 5_000;
const IDLE_SLEEP: Duration = Duration::from_millis(5);
const MAX_LOG: usize = 40;

/// Shared between the mining loop and the browser screen.
pub struct Shared {
    pub runner: Mutex<Runner>,
    pub mining: AtomicBool,
    pub hashrate: AtomicU64,
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
    let directory = match args.value("data") {
        Some(data) => PathBuf::from(data),
        None => olc_board::control_key::config_folder()?.join("node"),
    };
    start(args, directory, Some(App::new(args)?))
}

fn start(args: &Args, directory: PathBuf, app: Option<Arc<App>>) -> Result<(), String> {
    let coin = coinfile::load(args.value("coin"))?;
    let port: u16 = args.number("port", 7000)?;
    let web_port: u16 = args.number("web", 8000)?;
    let hash_limit: u64 = args.number("hash-limit", 0)?;

    let secret = load_or_create_secret(&directory)?;
    let wallet = Wallet::from_secret(secret).map_err(|_| "The key file holds a bad key")?;
    let plugins = Plugins {
        consensus: Box::new(ProofOfWork),
        languages: olc_lang::all(),
    };
    let mut engine = Engine::new(coin, plugins, wallet);
    let (mut store, saved) = Store::open(&directory)?;
    for block in saved {
        engine.restore_block(block);
    }

    let mut runner = Runner::new(engine);
    let tcp = TcpLink::new();
    tcp.listen(&format!("0.0.0.0:{port}"))
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
        runner: Mutex::new(runner),
        mining: AtomicBool::new(args.has("mine")),
        hashrate: AtomicU64::new(0),
        log: Mutex::new(VecDeque::new()),
        siblings,
        app,
    });
    crate::app::watch_ports(&shared);
    let host = if args.has("web-public") {
        "0.0.0.0"
    } else {
        "127.0.0.1"
    };
    web::serve(shared.clone(), format!("{host}:{web_port}"))?;
    println!("http://127.0.0.1:{web_port}");
    main_loop(&shared, &mut store, hash_limit);
    Ok(())
}

fn main_loop(shared: &Shared, store: &mut Store, hash_limit: u64) {
    let mut window = Instant::now();
    let mut hashes = 0u64;
    loop {
        let elapsed_ms = window.elapsed().as_millis() as u64;
        let allowed = hash_limit == 0 || hashes < hash_limit * elapsed_ms / 1_000;
        let mining = shared.mining.load(Ordering::Relaxed) && allowed;
        if let Ok(mut runner) = shared.runner.lock() {
            let now = unix_ms();
            runner.step(now);
            if mining {
                runner.engine.mine(MINING_BATCH, now);
                hashes += u64::from(MINING_BATCH);
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
