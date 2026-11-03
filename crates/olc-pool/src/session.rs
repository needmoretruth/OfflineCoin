//! One connection's lines, in and out. A line that is not JSON, a method nobody knows and a share
//! of the wrong shape each get an error and change nothing; a connection that keeps sending them is
//! closed.
use crate::pool::{Pool, Session, Stats, Submitted};
use crate::stratum::{
    self, answer, hex, notification, read_word, refused, unhex, Refusal, EXTRANONCE1_BYTES, EXTRANONCE2_BYTES,
    VERSION_ROLLING_MASK,
};
use crate::vardiff::Vardiff;
use olc_core::Address;
use olc_wallet::parse_address;
use serde_json::{json, Value};

/// Connections at once. Each costs a thread and a few kilobytes.
pub const MOST_SESSIONS: usize = 256;
/// Names one connection may sign in, as a proxy for a few miners would.
const MOST_NAMES: usize = 16;
/// The longest worker name after the address, in characters.
const MOST_NAME_CHARS: usize = 64;
/// Wrong lines a connection may send before it is closed.
pub const MOST_WRONG: u32 = 50;

/// What a line from a miner leads to.
pub struct Reply {
    pub lines: Vec<String>,
    /// False when the connection should close.
    pub keep: bool,
}

/// Reads `address.worker`, or a bare address, as every Stratum pool takes its user name.
pub fn worker_address(name: &str) -> Result<Address, &'static str> {
    let (address, worker) = name.split_once('.').unwrap_or((name, ""));
    if worker.chars().count() > MOST_NAME_CHARS || name.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err("The worker's name is too long or has spaces.");
    }
    parse_address(address).ok_or("The user name must start with an OLC address.")
}

impl Pool {
    /// A new connection, or `None` when the pool is full.
    pub fn open(&mut self, now_ms: u64) -> Option<u64> {
        if self.sessions.len() >= MOST_SESSIONS {
            return None;
        }
        let id = self.next_session;
        self.next_session += 1;
        // Each connection's extranonce 1 is its own, so no two hash the same coinbase.
        let extranonce1 = (id as u32).to_be_bytes();
        let session = Session {
            extranonce1,
            subscribed: false,
            names: Default::default(),
            vardiff: Vardiff::new(self.first_difficulty(), now_ms),
            rolling: 0,
            sent_job: None,
            sent_difficulty: None,
            malformed: 0,
        };
        self.sessions.insert(id, session);
        Some(id)
    }

    pub fn close(&mut self, id: u64) {
        let Some(session) = self.sessions.remove(&id) else {
            return;
        };
        for name in session.names.keys() {
            if let Some(stats) = self.workers.get_mut(name) {
                stats.sessions = stats.sessions.saturating_sub(1);
            }
        }
    }

    /// Answers one line from connection `id`.
    pub fn handle(&mut self, id: u64, line: &str, now_ms: u64) -> Reply {
        let message: Option<Value> = serde_json::from_str(line.trim()).ok();
        let method = message.as_ref().and_then(|message| message.get("method")?.as_str().map(str::to_string));
        let request = message.as_ref().map(|message| message.get("id").cloned().unwrap_or(Value::Null));
        let (Some(message), Some(request)) = (message.as_ref(), request) else {
            return self.wrong(id, &Value::Null, Refusal::Malformed);
        };
        let params = message.get("params").cloned().unwrap_or(Value::Array(Vec::new()));
        let Some(method) = method else {
            // An answer to something the pool asked needs nothing; anything else is not Stratum.
            if message.get("result").is_some() {
                return Reply { lines: Vec::new(), keep: true };
            }
            return self.wrong(id, &request, Refusal::Malformed);
        };
        let mut lines = match method.as_str() {
            "mining.subscribe" => self.subscribe(id, &request),
            "mining.authorize" => self.authorize(id, &request, &params, now_ms),
            "mining.configure" => self.configure(id, &request, &params),
            "mining.submit" => self.take_share(id, &request, &params, now_ms),
            "mining.extranonce.subscribe" => vec![answer(&request, json!(true))],
            "mining.suggest_difficulty" => self.suggest(id, &request, &params, now_ms),
            _ => return self.wrong(id, &request, Refusal::Other),
        };
        lines.extend(self.pushes(id, now_ms));
        let keep = self.sessions.get(&id).is_some_and(|session| session.malformed < MOST_WRONG);
        Reply { lines, keep }
    }

    fn wrong(&mut self, id: u64, request: &Value, refusal: Refusal) -> Reply {
        let Some(session) = self.sessions.get_mut(&id) else {
            return Reply { lines: Vec::new(), keep: false };
        };
        session.malformed += 1;
        Reply { lines: vec![refused(request, refusal)], keep: session.malformed < MOST_WRONG }
    }

    fn subscribe(&mut self, id: u64, request: &Value) -> Vec<String> {
        let Some(session) = self.sessions.get_mut(&id) else {
            return Vec::new();
        };
        session.subscribed = true;
        let tag = format!("{id:x}");
        let result = json!([
            [["mining.set_difficulty", tag], ["mining.notify", tag]],
            hex(&session.extranonce1),
            EXTRANONCE2_BYTES
        ]);
        vec![answer(request, result)]
    }

    fn authorize(&mut self, id: u64, request: &Value, params: &Value, now_ms: u64) -> Vec<String> {
        let name = params.get(0).and_then(Value::as_str).unwrap_or_default().to_string();
        let address = match worker_address(&name) {
            Ok(address) => address,
            Err(_) => return vec![refused(request, Refusal::Unauthorized)],
        };
        let Some(session) = self.sessions.get_mut(&id) else {
            return Vec::new();
        };
        if !session.names.contains_key(&name) {
            if session.names.len() >= MOST_NAMES {
                return vec![refused(request, Refusal::Unauthorized)];
            }
            session.names.insert(name.clone(), address);
            let difficulty = session.vardiff.difficulty();
            let stats = self.workers.entry(name).or_insert_with(|| Stats { address, first_ms: now_ms, ..Stats::default() });
            stats.sessions += 1;
            stats.difficulty = difficulty;
        }
        vec![answer(request, json!(true))]
    }

    /// BIP 310: a miner asks to roll version bits, and gets those BIP 320 leaves free.
    fn configure(&mut self, id: u64, request: &Value, params: &Value) -> Vec<String> {
        let asked = params.get(0).and_then(Value::as_array).cloned().unwrap_or_default();
        let wants_rolling = asked.iter().any(|name| name.as_str() == Some("version-rolling"));
        let options = params.get(1);
        let mask = options.and_then(|options| options.get("version-rolling.mask")?.as_str()).and_then(read_word);
        let Some(session) = self.sessions.get_mut(&id) else {
            return Vec::new();
        };
        if !wants_rolling {
            return vec![answer(request, json!({}))];
        }
        session.rolling = mask.unwrap_or(VERSION_ROLLING_MASK) & VERSION_ROLLING_MASK;
        let result = json!({ "version-rolling": true, "version-rolling.mask": stratum::word_hex(session.rolling) });
        vec![answer(request, result)]
    }

    fn suggest(&mut self, id: u64, request: &Value, params: &Value, now_ms: u64) -> Vec<String> {
        let asked = params.get(0).and_then(Value::as_f64).map(stratum::olc_difficulty);
        let (fixed, most) = (self.plan.fixed_difficulty, self.block_difficulty());
        if let (Some(asked), None, Some(session)) = (asked, fixed, self.sessions.get_mut(&id)) {
            session.vardiff.set(asked.clamp(1, most), now_ms);
        }
        vec![answer(request, json!(true))]
    }

    fn take_share(&mut self, id: u64, request: &Value, params: &Value, now_ms: u64) -> Vec<String> {
        let text = |at: usize| params.get(at).and_then(Value::as_str);
        let worker = text(0).unwrap_or_default().to_string();
        let parsed = (text(1), text(2).and_then(unhex), text(3).and_then(read_word), text(4).and_then(read_word));
        let version_bits = match text(5) {
            None => Some(None),
            Some(bits) => read_word(bits).map(Some),
        };
        let (Some(job), Some(extranonce2), Some(ntime), Some(nonce), Some(version_bits)) =
            (parsed.0, parsed.1, parsed.2, parsed.3, version_bits)
        else {
            let signed_in = self.sessions.get(&id).is_some_and(|session| session.names.contains_key(&worker));
            if let Some(stats) = self.workers.get_mut(&worker).filter(|_| signed_in) {
                stats.refused += 1;
            }
            if let Some(session) = self.sessions.get_mut(&id) {
                session.malformed += 1;
            }
            return vec![refused(request, Refusal::Malformed)];
        };
        let share = Submitted { job, extranonce2: &extranonce2, ntime, nonce, version_bits };
        match self.submit(id, &worker, &share, now_ms) {
            Ok(()) => vec![answer(request, json!(true))],
            Err(refusal) => vec![refused(request, refusal)],
        }
    }

    /// What the pool has to tell connection `id` now: a new difficulty, a new job.
    pub fn pushes(&mut self, id: u64, now_ms: u64) -> Vec<String> {
        let most = self.block_difficulty();
        let fixed = self.plan.fixed_difficulty;
        let newest = self.newest().map(|job| (job.number, job.notify.clone()));
        let Some(session) = self.sessions.get_mut(&id).filter(|session| session.subscribed) else {
            return Vec::new();
        };
        match fixed {
            Some(fixed) => drop(session.vardiff.set(fixed.clamp(1, most), now_ms)),
            None if session.vardiff.difficulty() > most => drop(session.vardiff.set(most, now_ms)),
            None => drop(session.vardiff.judge(now_ms, most)),
        }
        let mut lines = Vec::new();
        let difficulty = session.vardiff.difficulty();
        if session.sent_difficulty != Some(difficulty) {
            session.sent_difficulty = Some(difficulty);
            lines.push(notification("mining.set_difficulty", json!([stratum::stratum_difficulty(difficulty)])));
        }
        if let Some((number, mut notify)) = newest.filter(|(number, _)| session.sent_job != Some(*number)) {
            // A connection that has had no job yet starts clean, whatever the job says.
            notify.clean |= session.sent_job.is_none();
            session.sent_job = Some(number);
            lines.push(notification("mining.notify", notify.params()));
        }
        let names: Vec<String> = session.names.keys().cloned().collect();
        for name in names {
            if let Some(stats) = self.workers.get_mut(&name) {
                stats.difficulty = difficulty;
            }
        }
        lines
    }
}

// The pool's part and the miner's part of the extranonce fill the coinbase's slot exactly.
const _: () = assert!(EXTRANONCE1_BYTES + EXTRANONCE2_BYTES == olc_core::EXTRANONCE_BYTES);
