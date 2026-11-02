//! Small pieces of starting and showing one node.
use olc_core::{BlockHeader, CoinConfig, Plugins, Tx};
use olc_link_tcp::TcpLink;
use olc_node::{Card, Engine, Runner};
use olc_store::{load_headers, NodeSettings};
use olc_pow::ProofOfWork;
use olc_wallet::{format_address, parse_address};
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

pub struct Plan {
    pub coin: CoinConfig,
    pub folder: PathBuf,
    pub wallet_folder: PathBuf,
    pub bind_host: String,
    pub port: u16,
    pub port_tries: u16,
    pub peers: Vec<String>,
    pub cap: u64,
    pub mining: bool,
    pub card: Card,
    /// Bytes the chain may take on disk.
    pub storage: Option<u64>,
}

/// Puts `engine` in the node. Payments still waiting stay with it on the same coin: a person sent
/// them, and a change of settings must not take them back.
pub(super) fn carry_pool(runner: &mut Runner, engine: Engine) {
    let same_coin = runner.engine.chain().genesis_id() == engine.chain().genesis_id();
    let waiting: Vec<Tx> = runner.engine.pool().to_vec();
    runner.set_engine(engine);
    for tx in waiting.into_iter().filter(|_| same_coin) {
        runner.engine.send_signed(tx);
    }
}

/// An engine set the way this chain's folder says.
pub fn engine_with(mut engine: Engine, settings: &NodeSettings, folder: &Path) -> Engine {
    engine.set_miner(settings.miner.as_deref().and_then(parse_address));
    engine.set_note(settings.note.as_bytes());
    engine.set_merging(!settings.no_merge);
    for address in settings.watched.iter().filter_map(|text| parse_address(text)) {
        engine.watch(address);
    }
    if settings.light {
        engine.become_light(usize::MAX, &load_headers(folder));
    }
    engine
}

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
    listen_from(tcp, &plan.bind_host, plan.port, plan.port_tries)
}

/// Tries the ports in turn, then any port the system has free. A node that others cannot find by
/// its usual port can still be reached by the app that started it.
fn listen_from(tcp: &TcpLink, host: &str, first: u16, tries: u16) -> Option<u16> {
    (0..tries.max(1))
        .filter_map(|step| first.checked_add(step))
        .chain(std::iter::once(0))
        .find_map(|port| tcp.listen(&format!("{host}:{port}")).ok())
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn a_node_listens_even_when_its_usual_ports_are_taken() {
        let taken: Vec<_> = (0..3).map(|_| TcpListener::bind("127.0.0.1:0").unwrap()).collect();
        let first = taken[0].local_addr().unwrap().port();
        let tcp = TcpLink::new();
        let port = listen_from(&tcp, "127.0.0.1", first, 1).expect("a port");
        assert_ne!(port, first);
        assert_ne!(port, 0);
    }
}
