//! One simulated board: a real engine that answers a board's command lines.
use super::radio::RadioLink;
use olc_board::BoardLink;
use olc_control::{Beacon, BoardId};
use olc_core::coin::COIN;
use olc_core::{CoinConfig, Plugins};
use olc_node::{Card, Engine, Event, Hello, Runner};
use olc_pow::ProofOfWork;
use olc_wallet::{format_address, parse_address, Wallet};
use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// What a board does with no cap.
const FULL_RATE: u64 = 15_000;
const DEFAULT_CAP: u64 = 3_000;
const BUILD: &str = "sim-1";
const NEWER_BUILD: &str = "sim-2";

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn parse_hash(text: &str) -> Option<[u8; 32]> {
    if text.len() != 64 || !text.is_ascii() {
        return None;
    }
    let mut hash = [0u8; 32];
    for (index, byte) in hash.iter_mut().enumerate() {
        *byte = u8::from_str_radix(text.get(index * 2..index * 2 + 2)?, 16).ok()?;
    }
    Some(hash)
}

pub struct SimBoard {
    id: BoardId,
    name: String,
    runner: Runner,
    cap: u64,
    cap_chosen: bool,
    mining: bool,
    credit: f64,
    rate: u64,
    view: String,
    language: String,
    build: String,
    smooth: u32,
    start: Instant,
    /// What the board printed, for a link reading it.
    out: VecDeque<String>,
    dropped: BTreeMap<[u8; 32], String>,
}

impl SimBoard {
    pub fn new(index: u8, name: &str, coin: CoinConfig, radio: RadioLink, seeded: bool) -> Self {
        let mut secret = [index + 1; 32];
        secret[0] = 0x51;
        let wallet = Wallet::from_secret(secret).ok();
        let engine = wallet.map(|wallet| {
            let plugins = Plugins {
                consensus: Box::new(ProofOfWork),
                languages: olc_lang::all(),
            };
            Engine::new(coin, plugins, wallet)
        });
        let Some(engine) = engine else {
            unreachable!("the seed above is a valid key");
        };
        let mut runner = Runner::new(engine);
        runner.add_link(Box::new(radio));
        runner.card = Card {
            name: name.into(),
            device: "esp32-s3".into(),
            build: BUILD.into(),
            ..Card::default()
        };
        Self {
            id: [0xB0, 0, 0, 0, 0, index],
            name: name.into(),
            runner,
            cap: DEFAULT_CAP,
            cap_chosen: false,
            mining: seeded,
            credit: 0.0,
            rate: 0,
            view: "basic".into(),
            language: "en".into(),
            build: BUILD.into(),
            smooth: 10,
            start: Instant::now(),
            out: VecDeque::new(),
            dropped: BTreeMap::new(),
        }
    }

    pub fn id(&self) -> BoardId {
        self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn mark_updated(&mut self) {
        self.build = NEWER_BUILD.into();
        self.runner.card.build = NEWER_BUILD.into();
    }

    fn cap_now(&self) -> u64 {
        match self.cap {
            0 => FULL_RATE,
            cap => cap.min(FULL_RATE),
        }
    }

    pub fn step(&mut self, step: Duration) {
        let now = self.start.elapsed().as_millis() as u64;
        self.runner.step(now);
        self.rate = 0;
        if self.mining {
            self.credit += self.cap_now() as f64 * step.as_secs_f64();
            let batch = self.credit as u32;
            self.credit -= f64::from(batch);
            if batch > 0 {
                self.runner.engine.mine(batch, now);
            }
            self.rate = self.cap_now();
        }
        while let Some(event) = self.runner.engine.take_event() {
            if let Event::TxDropped { id, reason } = event {
                let reason = reason.map_or("full".to_string(), |why| format!("{why:?}"));
                self.dropped.insert(id, reason);
            }
        }
        self.runner.card.mining = self.mining;
        self.runner.card.hashrate = self.rate;
    }

    pub fn beacon(&self) -> Beacon {
        let status = self.runner.engine.status();
        Beacon {
            name: self.name.clone(),
            address: self.runner.engine.address(),
            height: status.height,
            tip: status.tip,
            balance: status.balance,
            hashrate: self.rate as u32,
            limit: self.cap as u32,
            peers: status.peers as u16,
            pool: status.pool as u16,
            mining: self.mining,
            offline: self.runner.offline,
            view: self.view.clone(),
            language: self.language.clone(),
            build: self.build.clone(),
            trusts_a_key: true,
            ..Beacon::default()
        }
    }

    pub fn hello(&self) -> Hello {
        let chain = self.runner.engine.chain();
        Hello {
            coin: chain.config().clone(),
            tip: chain.tip(),
            height: chain.height(),
            work: chain.total_work(),
            address: self.runner.engine.address(),
            card: self.runner.card.clone(),
        }
    }

    fn status_line(&self) -> String {
        let status = self.runner.engine.status();
        format!(
            "status height={} tip={} work={} difficulty={} balance={:.2} peers={} pool={} \
             hashrate={} limit={} chosen={} mining={} offline={} view={} lang={} address={}",
            status.height,
            hex(&status.tip),
            status.work,
            status.difficulty,
            status.balance as f64 / COIN as f64,
            status.peers,
            status.pool,
            self.rate,
            self.cap,
            self.cap_chosen,
            self.mining,
            self.runner.offline,
            self.view,
            self.language,
            format_address(&self.runner.engine.address()),
        )
    }

    fn pay(&mut self, words: &[&str]) -> String {
        let [address, amount] = words else {
            return "error usage".into();
        };
        let (Some(to), Ok(coins)) = (parse_address(address), amount.parse::<f64>()) else {
            return "error bad".into();
        };
        let units = (coins * COIN as f64) as u64;
        match self.runner.engine.pay(to, units, 0) {
            Ok(id) => format!("paid {}", hex(&id)),
            Err(error) => format!("error {error:?}"),
        }
    }

    fn tx(&self, id: &str) -> String {
        let hash = parse_hash(id).unwrap_or_default();
        let engine = &self.runner.engine;
        if let Some(height) = engine.chain().find_tx(&hash) {
            format!("tx {id} height={height}")
        } else if engine.in_pool(&hash) {
            format!("tx {id} pool")
        } else if let Some(reason) = self.dropped.get(&hash) {
            format!("tx {id} dropped={reason}")
        } else {
            format!("tx {id} none")
        }
    }

    /// Runs one line and returns what the board prints.
    pub fn command(&mut self, line: &str) -> String {
        let words: Vec<&str> = line.split_whitespace().collect();
        match words.as_slice() {
            ["status"] => return self.status_line(),
            ["pay", rest @ ..] => return self.pay(rest),
            ["tx", id] => return self.tx(id),
            ["limit", "default"] => {
                self.cap = DEFAULT_CAP;
                self.cap_chosen = false;
            }
            ["limit", cap] => {
                let Ok(cap) = cap.parse() else {
                    return "error bad".into();
                };
                self.cap = cap;
                self.cap_chosen = true;
            }
            ["mine", on] => self.mining = *on == "on",
            ["offline", on] => self.runner.offline = *on == "on",
            ["pool", "clear"] => self.runner.engine.clear_pool(),
            ["view", view] => self.view = (*view).into(),
            ["lang", language] => self.language = (*language).into(),
            ["name", name @ ..] => {
                self.name = name.join(" ");
                self.runner.card.name = self.name.clone();
            }
            ["smooth", frames] => {
                let Ok(frames) = frames.parse() else {
                    return "error bad".into();
                };
                self.smooth = frames;
            }
            _ => return format!("error unknown {line}"),
        }
        format!("ok {line}")
    }

    fn push_out(&mut self, line: String) {
        self.out.push_back(line);
    }
}

/// A board as a link: lines go in, printed lines come out.
pub struct SimLink {
    boards: Arc<Mutex<Vec<SimBoard>>>,
    id: BoardId,
}

impl SimLink {
    pub fn new(boards: Arc<Mutex<Vec<SimBoard>>>, id: BoardId) -> Self {
        Self { boards, id }
    }

    fn with<T>(&self, act: impl FnOnce(&mut SimBoard) -> T) -> Option<T> {
        let mut boards = self.boards.lock().ok()?;
        boards.iter_mut().find(|board| board.id == self.id).map(act)
    }
}

impl BoardLink for SimLink {
    fn send(&mut self, line: &str) -> Result<(), String> {
        for line in line.lines().filter(|line| !line.trim().is_empty()) {
            self.with(|board| {
                let answer = board.command(line);
                board.push_out(answer);
            })
            .ok_or("The board left.")?;
        }
        Ok(())
    }

    fn receive(&mut self, wait: Duration) -> Option<String> {
        let deadline = Instant::now() + wait;
        loop {
            if let Some(line) = self.with(|board| board.out.pop_front()).flatten() {
                return Some(line);
            }
            if Instant::now() >= deadline {
                return None;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
