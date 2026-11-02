//! Small pieces of starting and showing one node.
use super::host::Plan;
use olc_core::{BlockHeader, Plugins};
use olc_link_tcp::TcpLink;
use olc_node::Engine;
use olc_pow::ProofOfWork;
use olc_wallet::format_address;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

pub fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |time| time.as_millis() as u64)
}

/// How many hashes this computer does in a second on one core.
pub fn full_rate() -> u64 {
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

/// What this run goes by on the links. Nothing about it needs to last.
pub fn fresh_id() -> Result<u64, String> {
    let mut bytes = [0u8; 8];
    getrandom::fill(&mut bytes).map_err(|error| error.to_string())?;
    Ok(u64::from_le_bytes(bytes))
}

pub fn plugins() -> Plugins {
    Plugins {
        consensus: Box::new(ProofOfWork),
        languages: olc_lang::all(),
    }
}

pub fn listen_somewhere(tcp: &TcpLink, plan: &Plan) -> Option<u16> {
    (0..plan.port_tries.max(1))
        .filter_map(|step| plan.port.checked_add(step))
        .find_map(|port| tcp.listen(&format!("{}:{port}", plan.bind_host)).ok())
        .map(|address| address.port())
}

pub fn watched_view(engine: &Engine) -> Vec<(String, u64)> {
    let shown = engine.watched().into_iter();
    shown.map(|(address, coins)| (format_address(&address), coins)).collect()
}

pub fn stored_bytes(engine: &Engine, blocks: u64) -> u64 {
    let header = BlockHeader::ENCODED_LEN as u64;
    engine.light().map_or(blocks, |light| light.headers().held() as u64 * header)
}
