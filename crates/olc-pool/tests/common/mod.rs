//! Real engines and a miner that speaks to the pool line by line, as a connection would.
#![allow(dead_code)]
use olc_core::{Address, BlockHeader, Chain, CoinConfig, Plugins};
use olc_node::{Engine, Message, Outgoing};
use olc_pool::stratum::{hex, olc_difficulty, word_hex, Notify};
use olc_pool::Pool;
use olc_pow::ProofOfWork;
use olc_wallet::{format_address, Wallet};
use serde_json::{json, Value};

pub const BLOCK_DIFFICULTY: u64 = 800;

pub fn coin() -> CoinConfig {
    let mut coin = CoinConfig::olc();
    coin.name = "PoolProof".into();
    coin.ticker = "POP".into();
    coin.initial_difficulty = BLOCK_DIFFICULTY;
    coin.rules.min_difficulty = 1;
    coin.rules.coinbase_maturity = 2;
    coin
}

/// A moment after the test coin's genesis: block times step on from here, in the past.
pub fn start_ms() -> u64 {
    coin().genesis_time_ms + 60_000
}

/// The test coin started a moment ago, for engines that mine on the clock.
pub fn coin_now() -> CoinConfig {
    let mut coin = coin();
    coin.genesis_time_ms = olc_pool::unix_ms() / 1_000 * 1_000 - 5_000;
    coin
}

pub fn node(seed: u8) -> Engine {
    node_on(coin(), seed)
}

pub fn node_on(coin: CoinConfig, seed: u8) -> Engine {
    let plugins = Plugins {
        consensus: Box::new(ProofOfWork),
        languages: olc_lang::all(),
    };
    let chain = Chain::new(coin, plugins);
    let mut engine = Engine::on_chain(chain, Wallet::from_secret([seed; 32]).unwrap());
    engine.set_merging(false);
    engine
}

pub fn address(seed: u8) -> Address {
    Wallet::from_secret([seed; 32]).unwrap().address()
}

/// `address.worker`, as a miner signs in.
pub fn user(seed: u8, worker: &str) -> String {
    format!("{}.{worker}", format_address(&address(seed)))
}

/// Passes on the blocks and payments `from` wants to say, the way a link would.
pub fn relay(from: &mut Engine, to: &mut Engine) {
    while let Some(outgoing) = from.take_outgoing() {
        let (Outgoing::All { message, .. } | Outgoing::To(_, message)) = outgoing;
        if matches!(message, Message::Block(_) | Message::Tx(_)) {
            to.on_message(0, message);
        }
    }
    while to.take_outgoing().is_some() {}
}

/// Hands `to` every block of `from`'s chain it lacks, as a link would on the way to the tip.
pub fn catch_up(from: &Engine, to: &mut Engine) {
    for height in to.chain().height() + 1..=from.chain().height() {
        let block = from.chain().id_at(height).and_then(|id| from.chain().block(&id)).expect("a block");
        to.on_message(0, Message::Block(block));
    }
    while to.take_outgoing().is_some() {}
}

/// One connection to the pool, read and written as lines.
pub struct Miner {
    pub session: u64,
    pub user: String,
    pub extranonce1: Vec<u8>,
    pub notify: Option<Notify>,
    pub difficulty: u64,
    pub extranonce2: u32,
    pub next_id: u64,
    /// Every answer the pool gave a share: `true` or the error code.
    pub answers: Vec<Result<(), i64>>,
}

impl Miner {
    pub fn join(pool: &mut Pool, user: &str, now_ms: u64) -> Self {
        let session = pool.open(now_ms).expect("room in the pool");
        let mut miner = Self {
            session,
            user: user.to_string(),
            extranonce1: Vec::new(),
            notify: None,
            difficulty: 0,
            extranonce2: 0,
            next_id: 3,
            answers: Vec::new(),
        };
        miner.say(pool, &json!({"id": 1, "method": "mining.subscribe", "params": ["test"]}), now_ms);
        miner.say(pool, &json!({"id": 2, "method": "mining.authorize", "params": [user, "x"]}), now_ms);
        miner
    }

    /// Sends one line and reads what comes back.
    pub fn say(&mut self, pool: &mut Pool, message: &Value, now_ms: u64) -> Vec<Value> {
        let reply = pool.handle(self.session, &message.to_string(), now_ms);
        let lines: Vec<Value> = reply.lines.iter().map(|line| serde_json::from_str(line).unwrap()).collect();
        for line in &lines {
            self.read(line);
        }
        lines
    }

    pub fn listen(&mut self, pool: &mut Pool, now_ms: u64) {
        for line in pool.pushes(self.session, now_ms) {
            self.read(&serde_json::from_str(&line).unwrap());
        }
    }

    fn read(&mut self, line: &Value) {
        let params = &line["params"];
        match line["method"].as_str() {
            Some("mining.notify") => self.notify = Notify::from_params(params),
            Some("mining.set_difficulty") => self.difficulty = olc_difficulty(params[0].as_f64().unwrap()),
            _ if line["id"] == json!(1) => self.extranonce1 = olc_pool::stratum::unhex(line["result"][1].as_str().unwrap()).unwrap(),
            _ if line["id"].as_u64().is_some_and(|id| id >= 3) => {
                let answer = match line["result"].as_bool() {
                    Some(true) => Ok(()),
                    _ => Err(line["error"][0].as_i64().unwrap_or(-1)),
                };
                self.answers.push(answer);
            }
            _ => {}
        }
    }

    /// The header for the next extranonce 2.
    pub fn header(&mut self) -> BlockHeader {
        self.extranonce2 += 1;
        let notify = self.notify.as_ref().expect("a job");
        let extranonce = [self.extranonce1.as_slice(), &self.extranonce2.to_be_bytes()].concat();
        notify.header(&extranonce, notify.ntime, 0, notify.version).expect("a header")
    }

    /// Grinds a share with `engine`'s own proof of work.
    pub fn find(&mut self, engine: &Engine) -> BlockHeader {
        let mut header = self.header();
        while !engine.grind(&mut header, self.difficulty, 1_000) {}
        header
    }

    pub fn submission(&mut self, header: &BlockHeader) -> Value {
        let notify = self.notify.as_ref().expect("a job");
        let id = self.next_id;
        self.next_id += 1;
        let time = u32::try_from(header.time_ms / 1_000).unwrap();
        let params = json!([
            self.user,
            notify.job_id,
            hex(&self.extranonce2.to_be_bytes()),
            word_hex(time),
            word_hex(header.nonce)
        ]);
        json!({"id": id, "method": "mining.submit", "params": params})
    }

    /// Sends `header` as a share. The pool's answer, `Ok` or its error code.
    pub fn submit(&mut self, pool: &mut Pool, header: &BlockHeader, now_ms: u64) -> Result<(), i64> {
        let message = self.submission(header);
        self.say(pool, &message, now_ms);
        *self.answers.last().expect("an answer")
    }
}
