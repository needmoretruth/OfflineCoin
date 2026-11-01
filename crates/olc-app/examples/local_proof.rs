//! Runs real node processes the way the Local Nodes page does and reports what they did, to show
//! the page's numbers hold up: three nodes held to the ESP32 preset mine and check each other's
//! blocks, one pays another, two are killed outright and rejoin, then two share a wallet.
//!
//! `local_proof FOLDER PROGRAM MINUTES`
use olc_app::core::{format_coins, save_coin};
use olc_app::local::{address_of, ChildStatus, LocalView, NodeSpec, Preset, Supervisor, WalletChoice};
use olc_app::settings::Paths;
use olc_core::coin::COIN;
use olc_core::{Address, Block, Chain, CoinConfig, Plugins};
use olc_pow::ProofOfWork;
use olc_store::{load_or_create_secret, Store};
use olc_wallet::{format_address, Wallet};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

const SAMPLE: Duration = Duration::from_secs(5);
const SETTLE: Duration = Duration::from_secs(30);
const NEW_COIN_DIFFICULTY: u64 = 30_000;
const SHARE_RUN: Duration = Duration::from_secs(120);

#[derive(Default)]
struct Tally {
    rate: Vec<u64>,
    peak_rss: u64,
    peak_cgroup: u64,
    cpu: Vec<f64>,
}

impl Tally {
    fn add(&mut self, view: &LocalView, count_rate: bool) {
        if let Some(measured) = view.measured {
            self.peak_rss = self.peak_rss.max(measured.rss_bytes);
            self.peak_cgroup = self.peak_cgroup.max(measured.cgroup_bytes.unwrap_or(0));
            self.cpu.push(measured.cpu_percent);
        }
        if let Some(status) = view.status.as_ref().filter(|_| count_rate) {
            self.rate.push(status.hashrate);
        }
    }

    fn mean_rate(&self) -> u64 {
        self.rate.iter().sum::<u64>() / self.rate.len().max(1) as u64
    }
}

fn new_coin(name: &str) -> CoinConfig {
    let mut coin = CoinConfig::olc();
    coin.name = name.into();
    coin.ticker = "PRF".into();
    coin.message = format!("{name} genesis");
    coin.initial_difficulty = NEW_COIN_DIFFICULTY;
    coin
}

/// The kernel's own record of a process's limits, when it has a scope of its own.
fn limits(pid: u32) -> String {
    let group = std::fs::read_to_string(format!("/proc/{pid}/cgroup")).unwrap_or_default();
    let path = group.trim().trim_start_matches("0::").trim_start_matches('/').to_string();
    let read = |name: &str| {
        let file = Path::new("/sys/fs/cgroup").join(&path).join(name);
        std::fs::read_to_string(file).unwrap_or_default().trim().to_string()
    };
    format!("memory.max {} cpu.max {}", read("memory.max"), read("cpu.max"))
}

fn kill(pid: u32) {
    let _ = Command::new("kill").args(["-9", &pid.to_string()]).status();
}

struct Run {
    supervisor: Supervisor,
    specs: Vec<NodeSpec>,
    coin: PathBuf,
    tally: BTreeMap<u32, Tally>,
    started: Instant,
}

impl Run {
    fn peers(&self, spec: &NodeSpec) -> Vec<String> {
        self.specs
            .iter()
            .filter(|other| other.id != spec.id)
            .map(address_of)
            .collect()
    }

    fn start(&mut self, spec: &NodeSpec) {
        let peers = self.peers(spec);
        self.supervisor.start(spec, &self.coin, &peers).expect("start");
    }

    fn sample(&mut self) -> Vec<LocalView> {
        self.supervisor.reap();
        let settled = self.started.elapsed() > SETTLE;
        let views: Vec<LocalView> = self.specs.iter().map(|spec| self.supervisor.view(spec)).collect();
        for view in &views {
            self.tally.entry(view.spec.id).or_default().add(view, settled);
            let status = view.status.clone().unwrap_or_default();
            println!(
                "{:5}s {} pid={:?} height={} peers={} rate={} mined={} rss={}K cg={}K",
                self.started.elapsed().as_secs(),
                view.spec.name,
                view.pid,
                status.height,
                status.peers,
                status.hashrate,
                status.blocks_mined,
                view.measured.map_or(0, |m| m.rss_bytes / 1024),
                view.measured.and_then(|m| m.cgroup_bytes).unwrap_or(0) / 1024,
            );
        }
        views
    }

    fn status(&mut self, id: u32) -> ChildStatus {
        let spec = self.specs.iter().find(|spec| spec.id == id).cloned();
        spec.and_then(|spec| self.supervisor.view(&spec).status).unwrap_or_default()
    }

    /// Samples until `until` from the start of the run.
    fn run_until(&mut self, until: Duration) {
        while self.started.elapsed() < until {
            thread::sleep(SAMPLE);
            self.sample();
        }
    }

    fn kill_and_rejoin(&mut self, id: u32) {
        let spec = self.specs.iter().find(|spec| spec.id == id).cloned().expect("spec");
        let pid = self.supervisor.pid(id).expect("running");
        let height = self.status(id).height;
        println!("KILL {} pid {pid} at height {height}", spec.name);
        kill(pid);
        let gone = Instant::now();
        while self.supervisor.pid(id).is_some() {
            thread::sleep(Duration::from_millis(50));
            self.supervisor.reap();
        }
        println!("  noticed after {} ms", gone.elapsed().as_millis());
        self.start(&spec);
        let back = Instant::now();
        loop {
            thread::sleep(SAMPLE);
            self.sample();
            let others = self
                .specs
                .clone()
                .iter()
                .filter(|other| other.id != id)
                .map(|other| self.status(other.id).tip)
                .collect::<Vec<_>>();
            let mine = self.status(id);
            if others.contains(&mine.tip) && !mine.tip.is_empty() && mine.peers > 0 {
                println!("  rejoined at height {} after {} s", mine.height, back.elapsed().as_secs());
                return;
            }
        }
    }

    fn wait_spendable(&mut self, id: u32, amount: u64) {
        while self.status(id).spendable < amount {
            thread::sleep(SAMPLE);
            self.sample();
        }
    }

    fn pay(&mut self, from: u32, to: u32, coins: u64) -> String {
        let address = self.status(to).address;
        self.supervisor.send(from, &format!("pay {address} {coins}")).expect("pay");
        loop {
            thread::sleep(Duration::from_millis(200));
            for (id, answer) in self.supervisor.take_answers() {
                assert_eq!(id, from);
                assert_eq!(answer.error, None);
                return answer.paid.unwrap_or_default();
            }
        }
    }
}

/// Checks a node's saved blocks from nothing, as a new node would, and counts who mined them.
fn replay(coin: &CoinConfig, folder: &Path, paid: Option<&str>) -> (u64, u128, BTreeMap<String, u64>, Option<u64>) {
    let (_, blocks) = Store::open(folder).expect("store");
    let plugins = Plugins {
        consensus: Box::new(ProofOfWork),
        languages: olc_lang::all(),
    };
    let mut chain = Chain::new(coin.clone(), plugins);
    for block in blocks {
        chain.add_block(block).expect("every saved block is valid");
    }
    let mut miners: BTreeMap<String, u64> = BTreeMap::new();
    let mut found = None;
    let best: Vec<Block> = chain.best_chain_from(1).into_iter().cloned().collect();
    for (index, block) in best.iter().enumerate() {
        let lock: Address = block.txs[0].outputs[0].lock.as_slice().try_into().expect("address");
        *miners.entry(format_address(&lock)).or_default() += 1;
        for tx in &block.txs {
            let id: String = tx.id().iter().map(|byte| format!("{byte:02x}")).collect();
            if Some(id.as_str()) == paid {
                found = Some(index as u64 + 1);
            }
        }
    }
    (chain.height(), chain.total_work(), miners, found)
}

fn main() {
    let words: Vec<String> = std::env::args().skip(1).collect();
    let root = PathBuf::from(&words[0]);
    let program = PathBuf::from(&words[1]);
    let minutes: u64 = words[2].parse().expect("minutes");
    let total = Duration::from_secs(minutes * 60);
    let paths = Paths { root: root.clone() };

    let coin = new_coin("Proof");
    let file = save_coin(&paths, &coin).expect("coin");
    let specs: Vec<NodeSpec> = (0..3).map(|id| Preset::Esp32S3.spec(id)).collect();
    let mut run = Run {
        supervisor: Supervisor::with_program(paths.clone(), program.clone()),
        specs: specs.clone(),
        coin: file,
        tally: BTreeMap::new(),
        started: Instant::now(),
    };
    specs.iter().for_each(|spec| run.start(spec));
    thread::sleep(Duration::from_secs(1));
    for spec in &specs {
        println!("{}: {}", spec.name, limits(run.supervisor.pid(spec.id).unwrap_or(0)));
    }
    run.run_until(total / 4);
    run.wait_spendable(0, COIN);
    let paid = run.pay(0, 1, 1);
    println!("PAID {paid} at {} s", run.started.elapsed().as_secs());
    run.run_until(total / 2);
    run.kill_and_rejoin(2);
    run.run_until(total * 3 / 4);
    run.kill_and_rejoin(0);
    run.run_until(total);
    let final_views = run.sample();
    let tips: Vec<String> = final_views
        .iter()
        .map(|view| view.status.clone().unwrap_or_default().tip)
        .collect();
    let seconds = run.started.elapsed().as_secs();
    run.supervisor.stop_all();
    thread::sleep(Duration::from_secs(3));

    println!("--- result after {seconds} s, tips equal: {}", tips.iter().all(|tip| *tip == tips[0]));
    for view in &final_views {
        let tally = &run.tally[&view.spec.id];
        let status = view.status.clone().unwrap_or_default();
        let folder = paths.local_node(view.spec.id).join(run.coin.parent().and_then(Path::file_name).unwrap_or_default());
        let (height, work, miners, found) = replay(&coin, &folder, Some(&paid));
        let mean_cpu = tally.cpu.iter().sum::<f64>() / tally.cpu.len().max(1) as f64;
        println!(
            "{}: cap {} mean rate {} ({:+.1}%), peak rss {:.1} MiB, peak cgroup {:.1} MiB of limit {:.1} MiB, cpu {:.1}%, stored {} of {} bytes",
            view.spec.name,
            view.spec.hash_cap,
            tally.mean_rate(),
            (tally.mean_rate() as f64 / view.spec.hash_cap as f64 - 1.0) * 100.0,
            tally.peak_rss as f64 / 1048576.0,
            tally.peak_cgroup as f64 / 1048576.0,
            view.memory_limit as f64 / 1048576.0,
            mean_cpu,
            status.stored_bytes,
            view.spec.disk_mib * 1048576,
        );
        println!(
            "  replayed height {height} work {work}, {:.0} H/s of chain work over the run, paid tx in block {found:?}, balance {}, by miner {miners:?}",
            work as f64 / seconds as f64,
            format_coins(status.balance),
        );
    }

    share_run(&root.join("share"), &program);
    cpu_share_run(&root.join("cpu"), &program);
}

/// With no hash cap the CPU share alone sets the rate, as on a Raspberry Pi preset.
fn cpu_share_run(root: &Path, program: &Path) {
    let paths = Paths { root: root.to_path_buf() };
    let file = save_coin(&paths, &new_coin("Share of CPU")).expect("coin");
    let spec = NodeSpec {
        hash_cap: 0,
        cpu_percent: 20,
        ..Preset::RaspberryPi4.spec(20)
    };
    let mut run = Run {
        supervisor: Supervisor::with_program(paths.clone(), program.to_path_buf()),
        specs: vec![spec.clone()],
        coin: file,
        tally: BTreeMap::new(),
        started: Instant::now(),
    };
    run.start(&spec);
    thread::sleep(Duration::from_secs(1));
    println!("--- cpu share run: {}", limits(run.supervisor.pid(spec.id).unwrap_or(0)));
    run.run_until(Duration::from_secs(25));
    let views = run.sample();
    let view = &views[0];
    println!(
        "given 20% of a core, measured {:.1}% at {} H/s",
        view.measured.map_or(0.0, |used| used.cpu_percent),
        view.status.clone().unwrap_or_default().hashrate,
    );
    run.supervisor.stop_all();
    thread::sleep(Duration::from_secs(3));
}

/// Two nodes mining to one wallet, and two with their own, on a coin of their own.
fn share_run(root: &Path, program: &Path) {
    let paths = Paths { root: root.to_path_buf() };
    let coin = new_coin("Share");
    let file = save_coin(&paths, &coin).expect("coin");
    let shared_key = load_or_create_secret(&paths.wallet()).expect("wallet");
    let shared = format_address(&Wallet::from_secret(shared_key).expect("key").address());
    let specs: Vec<NodeSpec> = (10..14)
        .map(|id| NodeSpec {
            wallet: if id < 12 { WalletChoice::Shared } else { WalletChoice::Own },
            ..Preset::Esp32S3.spec(id)
        })
        .collect();
    let mut run = Run {
        supervisor: Supervisor::with_program(paths.clone(), program.to_path_buf()),
        specs: specs.clone(),
        coin: file,
        tally: BTreeMap::new(),
        started: Instant::now(),
    };
    specs.iter().for_each(|spec| run.start(spec));
    run.run_until(SHARE_RUN);
    let views = run.sample();
    run.supervisor.stop_all();
    thread::sleep(Duration::from_secs(3));
    println!("--- share run, the shared wallet is {shared}");
    for view in &views {
        let status = view.status.clone().unwrap_or_default();
        println!(
            "{} {:?}: address {}, its own blocks {}, balance {}",
            view.spec.name, view.spec.wallet, status.address, status.blocks_mined, format_coins(status.balance),
        );
    }
    let folder = paths.local_node(10).join(run.coin.parent().and_then(Path::file_name).unwrap_or_default());
    println!("by miner {:?}", replay(&coin, &folder, None).2);
}
