//! Someone on the network tries to pass as a board, against a real node on a real socket and the
//! real fleet. Beacons carry no signature, so the only proof of a board is the link it came by.
use super::host::{HeardFrom, Host, Plan};
use crate::fleet::{BoardRow, FleetPort, LineError, StartError};
use olc_board::control_key::ControlKey;
use olc_board::fleet::Fleet;
use olc_board::{Args, Extra};
use olc_control::{Beacon, BoardId, Body, Packet};
use olc_core::CoinConfig;
use olc_node::frame::encode;
use olc_node::network::{stamp, CONTROL, OPEN};
use olc_node::{Card, Hello, Message, Runner};
use std::io::Write;
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const BOARD: BoardId = [7; 6];
/// A peer number on the link after TCP, which is where a USB board comes in.
const CABLE: u32 = 1 << 24;
const TCP_PORT: u16 = 7360;

/// Hears what a runner hears, the way the manager does.
struct Listening(Mutex<Fleet>);

impl FleetPort for Listening {
    fn attach(&self, _runner: Arc<Mutex<Runner>>) {}
    fn pump(&self, runner: &mut Runner) {
        while let Some((peer, bytes)) = runner.take_control() {
            if let Ok(mut fleet) = self.0.lock() {
                fleet.hear(peer, &bytes);
            }
        }
    }
    fn board_rows(&self) -> Vec<BoardRow> {
        Vec::new()
    }
    fn new_boards(&self) -> Vec<String> {
        Vec::new()
    }
    fn job(&self, _key: &str) -> Option<String> {
        None
    }
    fn run_line(&self, _board: BoardId, _line: &str) -> Result<(), LineError> {
        Ok(())
    }
    fn start_update(&self, _board: BoardId) {}
    fn start_install(&self, _path: &str, _name: &str) -> bool {
        false
    }
    fn start_extra(
        &self,
        _extra: &'static dyn Extra,
        _boards: &[BoardId],
        _args: Args,
    ) -> Result<(), StartError> {
        Ok(())
    }
    fn run_log(&self) -> (Vec<String>, bool) {
        (Vec::new(), false)
    }
    fn stop_extra(&self) {}
}

const SECRET: [u8; 32] = [3; 32];

fn beacon() -> Vec<u8> {
    let beacon = Beacon {
        name: "not-a-board".into(),
        address: olc_crypto::address_of(&olc_crypto::public_key(&SECRET).expect("a valid key")),
        height: 9,
        tip: [0; 32],
        balance: 0,
        hashrate: 1,
        limit: 0,
        peers: 0,
        pool: 0,
        mining: true,
        offline: false,
        view: "basic".into(),
        language: "en".into(),
        build: "x".into(),
        slot: 0,
        counter: 41,
        trusts_a_key: true,
        refused: 0,
        ..Beacon::default()
    };
    let signed = olc_control::sign_beacon(&SECRET, &BOARD, beacon).expect("a valid key");
    Packet {
        from: BOARD,
        to: None,
        id: 1,
        hops: 0,
        body: Body::Beacon(signed),
    }
    .encode()
}

fn folder(name: &str) -> PathBuf {
    let folder = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../.scratch/tests")
        .join(name);
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).unwrap();
    folder
}

fn host(root: &Path, fleet: Arc<Listening>, port: u16) -> Host {
    let plan = Plan {
        coin: CoinConfig::olc(),
        folder: root.join("chain"),
        wallet_folder: root.join("wallet"),
        bind_host: "127.0.0.1".into(),
        port,
        port_tries: 1,
        peers: Vec::new(),
        cap: 0,
        mining: false,
        card: Card::default(),
        storage: None,
    };
    Host::start(plan, Some(fleet)).unwrap()
}

fn connect(port: u16) -> TcpStream {
    for _ in 0..100 {
        if let Ok(stream) = TcpStream::connect(("127.0.0.1", port)) {
            return stream;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("the node never listened");
}

#[test]
fn a_tcp_peer_cannot_make_a_board_row() {
    let root = folder("forged-beacon");
    let key = ControlKey::load_or_create(&root).unwrap();
    let fleet = Arc::new(Listening(Mutex::new(Fleet::new(key, Runner::link_of).unwrap())));
    let node = host(&root, fleet.clone(), TCP_PORT);

    let packet = beacon();
    let framed = encode(&stamp(CONTROL, &Message::Control(packet.clone()).encode()));
    let mut forger = connect(TCP_PORT);
    forger.write_all(&framed).unwrap();
    std::thread::sleep(Duration::from_millis(600));
    assert!(fleet.0.lock().unwrap().boards().is_empty());

    // The same bytes make a row when they come in on a board's link, so the test proves refusal.
    fleet.0.lock().unwrap().hear(CABLE, &packet);
    assert_eq!(fleet.0.lock().unwrap().boards().len(), 1);
    drop(node);
}

#[test]
fn a_tcp_peer_that_hangs_up_is_not_taken_for_a_board() {
    let root = folder("forged-hello");
    let key = ControlKey::load_or_create(&root).unwrap();
    let fleet = Arc::new(Listening(Mutex::new(Fleet::new(key, Runner::link_of).unwrap())));
    let node = host(&root, fleet, TCP_PORT + 1);

    let hello = Hello {
        coin: CoinConfig::olc(),
        tip: [0; 32],
        height: 1,
        work: 1,
        address: [1; 20],
        card: Card::default(),
    };
    let framed = encode(&stamp(OPEN, &Message::Hello(Box::new(hello)).encode()));
    for _ in 0..20 {
        let mut forger = connect(TCP_PORT + 1);
        forger.write_all(&framed).unwrap();
        drop(forger);
    }
    let until = Instant::now() + Duration::from_millis(800);
    while Instant::now() < until {
        assert!(node.take_heard().iter().all(|(from, _)| *from != HeardFrom::Board));
        std::thread::sleep(Duration::from_millis(10));
    }
}
