//! A miner that knows Bitcoin and nothing of OLC joins the pool over TCP and gets a share
//! accepted. Its side is written from the Stratum text and Bitcoin's header layout alone, with
//! cpuminer's way of turning a difficulty into a target: no line of it comes from this repository.
mod common;

use common::{node, user};
use olc_pool::{unix_ms, Plan, Pool, Server};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn sha256d(bytes: &[u8]) -> [u8; 32] {
    let once = Sha256::digest(bytes);
    Sha256::digest(once).into()
}

fn from_hex(text: &str) -> Vec<u8> {
    (0..text.len() / 2).map(|at| u8::from_str_radix(&text[at * 2..at * 2 + 2], 16).unwrap()).collect()
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// cpuminer's `diff_to_target`: eight little-endian words, the most significant last.
fn diff_to_target(mut difficulty: f64) -> [u32; 8] {
    let mut target = [0u32; 8];
    let mut k = 6;
    while k > 0 && difficulty > 1.0 {
        difficulty /= 4_294_967_296.0;
        k -= 1;
    }
    let m = (4_294_901_760.0 / difficulty) as u64;
    if m == 0 && k == 6 {
        return [u32::MAX; 8];
    }
    target[k] = m as u32;
    target[k + 1] = (m >> 32) as u32;
    target
}

/// cpuminer's `fulltest`: the hash read as a little-endian number is at most the target.
fn fulltest(hash: &[u8; 32], target: &[u32; 8]) -> bool {
    for at in (0..8).rev() {
        let word = u32::from_le_bytes(hash[at * 4..at * 4 + 4].try_into().unwrap());
        if word != target[at] {
            return word < target[at];
        }
    }
    true
}

struct Job {
    id: String,
    prevhash: Vec<u8>,
    coinb1: Vec<u8>,
    coinb2: Vec<u8>,
    branch: Vec<Vec<u8>>,
    version: String,
    nbits: String,
    ntime: String,
}

/// Bitcoin's 80-byte header for a job: version, previous hash, Merkle root, time, bits, nonce,
/// numbers little-endian, as the Stratum text says to build it.
fn header(job: &Job, extranonce1: &[u8], extranonce2: &[u8], nonce: u32) -> Vec<u8> {
    let coinbase = [job.coinb1.as_slice(), extranonce1, extranonce2, job.coinb2.as_slice()].concat();
    let mut root = sha256d(&coinbase);
    for step in &job.branch {
        root = sha256d(&[root.as_slice(), step.as_slice()].concat());
    }
    let word = |text: &str| u32::from_str_radix(text, 16).unwrap().to_le_bytes();
    let mut previous = job.prevhash.clone();
    for four in previous.chunks_mut(4) {
        four.reverse();
    }
    [&word(&job.version)[..], &previous, &root, &word(&job.ntime), &word(&job.nbits), &nonce.to_le_bytes()].concat()
}

fn send(stream: &mut TcpStream, message: Value) {
    stream.write_all(format!("{message}\n").as_bytes()).unwrap();
}

#[test]
fn a_miner_that_knows_only_bitcoin_gets_a_share_accepted() {
    let engine = node(1);
    let pool = Arc::new(Mutex::new(Pool::new(engine.address(), Plan::default())));
    pool.lock().unwrap().refresh(&engine, unix_ms());
    let server = Server::start(SocketAddr::from(([127, 0, 0, 1], 0)), pool.clone()).unwrap();

    let mut stream = TcpStream::connect(server.address()).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    send(&mut stream, json!({"id": 1, "method": "mining.subscribe", "params": ["foreign/1.0"]}));
    send(&mut stream, json!({"id": 2, "method": "mining.authorize", "params": [user(2, "foreign"), "x"]}));

    let (mut extranonce1, mut extranonce2_size, mut difficulty, mut job) = (Vec::new(), 0, None, None);
    let mut answers: Vec<Value> = Vec::new();
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(60) {
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        let message: Value = serde_json::from_str(&line).unwrap();
        match message["method"].as_str() {
            Some("mining.set_difficulty") => difficulty = message["params"][0].as_f64(),
            Some("mining.notify") => {
                let params = &message["params"];
                let text = |at: usize| params[at].as_str().unwrap().to_string();
                let branch = params[4].as_array().unwrap().iter().map(|step| from_hex(step.as_str().unwrap())).collect();
                job = Some(Job {
                    id: text(0),
                    prevhash: from_hex(&text(1)),
                    coinb1: from_hex(&text(2)),
                    coinb2: from_hex(&text(3)),
                    branch,
                    version: text(5),
                    nbits: text(6),
                    ntime: text(7),
                });
            }
            _ if message["id"] == json!(1) => {
                extranonce1 = from_hex(message["result"][1].as_str().unwrap());
                extranonce2_size = message["result"][2].as_u64().unwrap() as usize;
            }
            _ if message["id"].as_u64().is_some() => answers.push(message.clone()),
            _ => {}
        }
        if answers.iter().any(|answer| answer["id"] == json!(3)) {
            break;
        }
        let (Some(difficulty), Some(job)) = (difficulty, job.as_ref()) else {
            continue;
        };
        if answers.iter().all(|answer| answer["id"] != json!(2)) {
            continue;
        }
        assert_eq!(answers[0]["result"], json!(true), "signed in");
        let target = diff_to_target(difficulty);
        let extranonce2 = vec![0u8; extranonce2_size];
        let nonce = (0u32..).find(|nonce| fulltest(&sha256d(&header(job, &extranonce1, &extranonce2, *nonce)), &target)).unwrap();
        let params = json!([user(2, "foreign"), job.id, to_hex(&extranonce2), job.ntime, format!("{nonce:08x}")]);
        send(&mut stream, json!({"id": 3, "method": "mining.submit", "params": params}));
    }
    let answer = answers.iter().find(|answer| answer["id"] == json!(3)).expect("an answer to the share");
    assert_eq!(answer["result"], json!(true), "{answer}");
    let rows = pool.lock().unwrap().workers(unix_ms());
    assert_eq!(rows[0].accepted, 1);
    drop(server);
}
