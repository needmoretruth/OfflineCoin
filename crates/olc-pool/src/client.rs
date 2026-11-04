//! Joining a pool, as any Stratum v1 miner does: subscribe, sign in as `address.worker`, take each
//! `mining.notify`, build the header and send back what reaches the difficulty. The hashing itself
//! happens elsewhere: this hands out a header and takes back the ones that met it.
use crate::stratum::{hex, olc_difficulty, request, word_hex, Notify};
use olc_core::{BlockHeader, Hash, Tx, ADDRESS_LANGUAGE, EXTRANONCE_BYTES};
use serde_json::{json, Value};
use std::collections::{BTreeMap, VecDeque};
use std::io::{ErrorKind, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const TURN: Duration = Duration::from_millis(100);
/// How often `settle` looks again.
const SETTLE_TURN: Duration = Duration::from_millis(2);
const DIAL_WITHIN: Duration = Duration::from_secs(5);
const AGAIN_AFTER: Duration = Duration::from_secs(5);
/// A pool that said nothing for this long is dialled again.
const QUIET_FOR: Duration = Duration::from_secs(180);
const MOST_LINE: usize = 64 * 1_024;
/// Headers handed out and not yet answered, by Merkle root.
const MOST_HANDED: usize = 256;
const MOST_JOBS: usize = 8;

/// Where a joined miner stands.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Link {
    #[default]
    Dialling,
    /// Connected and signed in, with work.
    Working,
    /// The pool did not take the worker's name.
    NameRefused,
    /// Nothing answers at that address.
    Unreachable,
    /// The pool's blocks do not fit OLC's: its extranonce or header is of another shape.
    Foreign,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ClientView {
    pub link: Link,
    pub accepted: u64,
    pub refused: u64,
    /// The share difficulty the pool asks, in OLC's hashes.
    pub difficulty: u64,
    /// The last reason the pool gave for refusing a share.
    pub last_refusal: Option<String>,
    /// Shares of a job the pool had already ended with a new block, kept back as Bitcoin's
    /// miners keep them: the pool could only refuse them.
    pub stale: u64,
    /// The address the pool's blocks pay, read from its newest job: the name its blocks go by.
    pub payee: Option<[u8; 20]>,
}

/// A header to hash and the difficulty a share must meet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Work {
    pub header: BlockHeader,
    pub difficulty: u64,
}

#[derive(Default)]
struct State {
    view: ClientView,
    extranonce1: Vec<u8>,
    extranonce2_size: usize,
    jobs: VecDeque<Notify>,
    generation: u64,
    handed: BTreeMap<Hash, (String, Vec<u8>)>,
    order: VecDeque<Hash>,
    outbox: Vec<String>,
    /// The connection's sending half while it is open: lines go out under this lock, whole.
    writer: Option<TcpStream>,
    /// Shares sent that the pool has not answered yet.
    unanswered: u64,
    next_id: u64,
}

impl State {
    /// Sends what waits, at once. False when the connection broke.
    fn flush(&mut self) -> bool {
        let Some(writer) = self.writer.as_mut() else {
            return true;
        };
        let lines = std::mem::take(&mut self.outbox);
        lines.iter().all(|line| writer.write_all(line.as_bytes()).is_ok())
    }
}

pub struct Client {
    state: Arc<Mutex<State>>,
    stop: Arc<AtomicBool>,
    done: Option<JoinHandle<()>>,
    user: String,
}

impl Client {
    /// Starts joining the pool at `address` (`host:port`) as `user` (`address.worker`).
    pub fn start(address: &str, user: &str, password: &str) -> Self {
        let state = Arc::new(Mutex::new(State { next_id: 10, ..State::default() }));
        let stop = Arc::new(AtomicBool::new(false));
        let (shared, watch) = (state.clone(), stop.clone());
        let (address, name, password) = (address.to_string(), user.to_string(), password.to_string());
        let done = thread::spawn(move || keep_joined(&address, &name, &password, &shared, &watch));
        Self { state, stop, done: Some(done), user: user.to_string() }
    }

    pub fn user(&self) -> &str {
        &self.user
    }

    pub fn view(&self) -> ClientView {
        self.state.lock().map(|state| state.view.clone()).unwrap_or_default()
    }

    /// Goes up whenever the pool sends new work or a new difficulty.
    pub fn generation(&self) -> u64 {
        self.state.lock().map_or(0, |state| state.generation)
    }

    /// A header for the newest job with `extranonce2`, or `None` before the first job.
    pub fn work(&self, extranonce2: u64) -> Option<Work> {
        let mut state = self.state.lock().ok()?;
        let notify = state.jobs.back()?.clone();
        let size = state.extranonce2_size;
        if state.extranonce1.len() + size != EXTRANONCE_BYTES {
            state.view.link = Link::Foreign;
            return None;
        }
        let rolled: Vec<u8> = extranonce2.to_be_bytes()[8 - size..].to_vec();
        let extranonce = [state.extranonce1.as_slice(), rolled.as_slice()].concat();
        let header = notify.header(&extranonce, notify.ntime, 0, notify.version)?;
        if state.handed.len() >= MOST_HANDED {
            if let Some(oldest) = state.order.pop_front() {
                state.handed.remove(&oldest);
            }
        }
        state.handed.insert(header.merkle_root, (notify.job_id.clone(), rolled));
        state.order.push_back(header.merkle_root);
        Some(Work { header, difficulty: state.view.difficulty.max(1) })
    }

    /// Sends a header that met the difficulty. One the pool never handed out is dropped.
    pub fn submit(&self, header: &BlockHeader) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        let Some((job, extranonce2)) = state.handed.get(&header.merkle_root).cloned() else {
            return;
        };
        // A new block's notice cleared the jobs before it: this share is of work already over.
        if !state.jobs.iter().any(|notify| notify.job_id == job) {
            state.view.stale += 1;
            return;
        }
        let time = u32::try_from(header.time_ms / 1_000).unwrap_or(u32::MAX);
        let params = json!([self.user, job, hex(&extranonce2), word_hex(time), word_hex(header.nonce)]);
        let id = state.next_id;
        state.next_id += 1;
        state.outbox.push(request(id, "mining.submit", params));
        if state.writer.is_some() {
            state.unanswered += 1;
        }
        // Sent now, not at the reader's next turn: a share only counts on the job it was found on.
        // A broken connection is the reader's to notice.
        let _ = state.flush();
    }

    /// Waits, at most `within`, until the pool answered every share sent, or work since `generation`
    /// came. True when it did.
    pub fn settle(&self, generation: Option<u64>, within: Duration) -> bool {
        let started = Instant::now();
        loop {
            let done = self.state.lock().map_or(true, |state| match generation {
                Some(before) => state.generation != before,
                None => state.unanswered == 0 || state.writer.is_none(),
            });
            if done {
                return true;
            }
            if started.elapsed() >= within {
                return false;
            }
            thread::sleep(SETTLE_TURN);
        }
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(done) = self.done.take() {
            let _ = done.join();
        }
    }
}

fn keep_joined(address: &str, user: &str, password: &str, state: &Mutex<State>, stop: &AtomicBool) {
    while !stop.load(Ordering::Relaxed) {
        let link = once(address, user, password, state, stop);
        if let Ok(mut state) = state.lock() {
            state.view.link = link;
            state.jobs.clear();
            state.generation += 1;
            // Shares of the connection that ended are of jobs no pool holds any more.
            state.writer = None;
            state.outbox.clear();
            state.unanswered = 0;
        }
        let wait = Instant::now();
        while wait.elapsed() < AGAIN_AFTER && !stop.load(Ordering::Relaxed) {
            thread::sleep(TURN);
        }
    }
}

fn dial(address: &str) -> Option<TcpStream> {
    let addresses = address.to_socket_addrs().ok()?;
    addresses.into_iter().find_map(|at| TcpStream::connect_timeout(&at, DIAL_WITHIN).ok())
}

/// One connection, until it ends. Gives how it ended.
fn once(address: &str, user: &str, password: &str, state: &Mutex<State>, stop: &AtomicBool) -> Link {
    let Some(mut stream) = dial(address) else {
        return Link::Unreachable;
    };
    if stream.set_read_timeout(Some(TURN)).is_err() || stream.set_write_timeout(Some(DIAL_WITHIN)).is_err() {
        return Link::Unreachable;
    }
    let hello = [
        request(1, "mining.subscribe", json!(["OLC Core"])),
        request(2, "mining.authorize", json!([user, password])),
    ];
    if hello.iter().any(|line| stream.write_all(line.as_bytes()).is_err()) {
        return Link::Unreachable;
    }
    let Ok(writer) = stream.try_clone() else {
        return Link::Unreachable;
    };
    if let Ok(mut state) = state.lock() {
        state.writer = Some(writer);
    }
    let mut buffer: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 4_096];
    let mut heard = Instant::now();
    while !stop.load(Ordering::Relaxed) {
        if !state.lock().is_ok_and(|mut state| state.flush()) {
            return Link::Unreachable;
        }
        match stream.read(&mut chunk) {
            Ok(0) => return Link::Unreachable,
            Ok(count) => {
                heard = Instant::now();
                buffer.extend_from_slice(&chunk[..count]);
            }
            Err(error) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
            Err(_) => return Link::Unreachable,
        }
        while let Some(end) = buffer.iter().position(|byte| *byte == b'\n') {
            let line: Vec<u8> = buffer.drain(..=end).collect();
            let message: Option<Value> = serde_json::from_slice(&line).ok();
            let Ok(mut state) = state.lock() else {
                return Link::Unreachable;
            };
            if let Some(ended) = message.and_then(|message| take(&mut state, &message)) {
                return ended;
            }
        }
        if buffer.len() > MOST_LINE || heard.elapsed() > QUIET_FOR {
            return Link::Unreachable;
        }
    }
    Link::Dialling
}

/// The address a job's coinbase pays most, as an explorer names a pool's block by it. The
/// extranonce only fills a data field, so zeros of its length read the same outputs.
fn payee_of(notify: &Notify) -> Option<[u8; 20]> {
    let coinbase: Tx = postcard::from_bytes(&notify.coinbase_bytes(&[0; EXTRANONCE_BYTES])).ok()?;
    let paid = coinbase.outputs.iter().filter(|output| output.language == ADDRESS_LANGUAGE);
    paid.max_by_key(|output| output.value).and_then(|output| <[u8; 20]>::try_from(output.lock.as_slice()).ok())
}

/// Takes one message from the pool. `Some` ends the connection.
fn take(state: &mut State, message: &Value) -> Option<Link> {
    let params = message.get("params").cloned().unwrap_or(Value::Null);
    match message.get("method").and_then(Value::as_str) {
        Some("mining.notify") => {
            let notify = Notify::from_params(&params)?;
            state.view.payee = payee_of(&notify).or(state.view.payee);
            if notify.clean {
                state.jobs.clear();
            }
            state.jobs.push_back(notify);
            while state.jobs.len() > MOST_JOBS {
                state.jobs.pop_front();
            }
            if state.view.link == Link::Dialling {
                state.view.link = Link::Working;
            }
            state.generation += 1;
            return None;
        }
        Some("mining.set_difficulty") => {
            let asked = params.get(0).and_then(Value::as_f64)?;
            state.view.difficulty = olc_difficulty(asked);
            state.generation += 1;
            return None;
        }
        Some(_) => return None,
        None => {}
    }
    let id = message.get("id").and_then(Value::as_u64)?;
    let result = message.get("result").cloned().unwrap_or(Value::Null);
    let error = message.get("error").filter(|error| !error.is_null());
    match id {
        1 => {
            let extranonce1 = result.get(1).and_then(Value::as_str).and_then(crate::stratum::unhex)?;
            let size = result.get(2).and_then(Value::as_u64)? as usize;
            if extranonce1.len() + size != EXTRANONCE_BYTES {
                return Some(Link::Foreign);
            }
            state.extranonce1 = extranonce1;
            state.extranonce2_size = size;
        }
        2 if result.as_bool() != Some(true) => return Some(Link::NameRefused),
        2 => {}
        _ if result.as_bool() == Some(true) => {
            state.unanswered = state.unanswered.saturating_sub(1);
            state.view.accepted += 1;
        }
        _ => {
            state.unanswered = state.unanswered.saturating_sub(1);
            state.view.refused += 1;
            let reason = error.and_then(|error| error.get(1)?.as_str().map(str::to_string));
            state.view.last_refusal = reason;
        }
    }
    None
}
