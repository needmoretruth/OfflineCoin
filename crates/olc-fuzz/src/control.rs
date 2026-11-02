//! What a board and the computer that runs it read from each other: packets anyone can send.
use crate::{Parse, Target};
use olc_board::control_key::ControlKey;
use olc_board::{BoardState, Fleet, Status};
use olc_control::gate::{allowed_remotely, mac_end, sign, Gate};
use olc_control::packet::{
    Action, Beacon, Body, BoardId, OldBeacon, Order, Packet, Read, Refusal, Signed,
};
use olc_control::router::Router;
use olc_control::sending::Sending;
use olc_control::update::{ImageSlot, Update};
use olc_control::{sign_beacon, SignedBeacon};
use olc_crypto::{address_of, Signer};

const ME: BoardId = [1; 6];
const OTHER: BoardId = [2; 6];
const SECRET: [u8; 32] = [9; 32];
const SESSION: u32 = 11;
const IMAGE_SIZE: u32 = 8_000;
const CAPACITY: u32 = 64_000;
const PAGE: usize = 4_096;

fn order(action: Action, counter: u64) -> Option<Signed> {
    sign(&SECRET, Order { board: ME, counter, action })
}

fn orders() -> Vec<Signed> {
    let hash = [7; 32];
    let actions = [
        Action::Line("status".into()),
        Action::Update { session: SESSION, size: IMAGE_SIZE, image: hash },
        Action::Burst { session: SESSION, size: IMAGE_SIZE, image: hash },
        Action::Serve { target: OTHER, session: SESSION, size: IMAGE_SIZE, image: hash },
    ];
    actions.into_iter().zip(1..).filter_map(|(action, counter)| order(action, counter)).collect()
}

fn bodies() -> Vec<Body> {
    let mut bodies = vec![
        Body::Read(Read::Status),
        Body::Read(Read::Tx([3; 32])),
        Body::Line("height=1".into()),
        Body::Chunk { session: SESSION, offset: 0, data: vec![5; 64] },
        Body::Progress { session: SESSION, next: 64 },
        Body::Refused(Refusal::Replayed),
        Body::Piece { session: SESSION, index: 3, data: vec![5; 64] },
        Body::AskMissing { session: SESSION },
        Body::Missing { session: SESSION, done: 64, runs: vec![(1, 2), (9, 1)] },
    ];
    bodies.extend(orders().into_iter().map(Body::Order));
    bodies
}

fn beacon() -> Option<SignedBeacon> {
    let signer = Signer::new(&SECRET)?;
    let address = address_of(signer.public_key());
    let beacon = Beacon { name: "board".into(), address, height: 3, ..Beacon::default() };
    sign_beacon(&signer, &OTHER, beacon)
}

/// As a build from before signed beacons wrote it: the same fields, one after another.
fn old_beacon() -> Vec<u8> {
    let start = (OTHER, None::<BoardId>, 3u32, 0u8, 0u32, "old");
    let numbers = ([1u8; 20], 3u64, [2u8; 32], 5u64, 6u32, 7u32, 1u16, 1u16);
    let words = (true, false, "coin", "ko", "b1");
    postcard::to_allocvec(&(start, numbers, words)).unwrap_or_default()
}

fn packets() -> Vec<Vec<u8>> {
    let packet = |to, hops, body| Packet { from: OTHER, to, id: 3, hops, body };
    let mut seeds = Vec::new();
    for body in bodies() {
        seeds.push(packet(Some(ME), 2, body.clone()).encode());
        seeds.push(packet(None, u8::MAX, body).encode());
    }
    if let Some(beacon) = beacon() {
        seeds.push(packet(None, 0, Body::Beacon(beacon.clone())).encode());
        seeds.push(packet(None, u8::MAX, Body::Beacon(beacon)).encode());
    }
    seeds.push(old_beacon());
    seeds
}

fn read_packet(bytes: &[u8]) {
    if let Some(packet) = Packet::decode(bytes) {
        crate::taken();
        std::hint::black_box(packet.encode());
    }
    if let Some(beacon) = OldBeacon::decode(bytes) {
        std::hint::black_box(beacon.encode());
        std::hint::black_box(beacon.route());
    }
}

fn start_router() -> Parse {
    let mut router = Router::new(ME, 1);
    Box::new(move |bytes| {
        if let Some(packet) = Packet::decode(bytes) {
            crate::taken();
            let peer = u32::from(bytes.first().copied().unwrap_or(0));
            std::hint::black_box(router.incoming(peer, &packet).forward);
        }
    })
}

fn signed_orders() -> Vec<Vec<u8>> {
    let encode = |signed: Signed| postcard::to_allocvec(&signed).unwrap_or_default();
    orders().into_iter().map(encode).collect()
}

fn start_gate() -> Parse {
    let mut gate = Gate::new(ME, olc_crypto::public_key(&SECRET), 0);
    Box::new(move |bytes| {
        if let Ok(signed) = postcard::from_bytes::<Signed>(bytes) {
            crate::taken();
            if gate.admit(&signed).is_ok() {
                crate::deep();
                gate = Gate::new(ME, olc_crypto::public_key(&SECRET), 0);
            }
        }
    })
}

fn read_gate_text(bytes: &[u8]) {
    let text = String::from_utf8_lossy(bytes);
    if allowed_remotely(&text) || mac_end(&text).is_some() {
        crate::taken();
    }
}

fn gate_lines() -> Vec<Vec<u8>> {
    let lines = ["status", "mine on", "mac aa:bb:cc:dd:ee:ff", "mac=aabbccddeeff", "update"];
    lines.iter().map(|line| line.as_bytes().to_vec()).collect()
}

#[derive(Default)]
struct Slot(Vec<u8>);

impl ImageSlot for Slot {
    fn capacity(&self) -> u32 {
        CAPACITY
    }

    fn write_page(&mut self, offset: u32, page: &[u8]) -> bool {
        let end = offset as usize + page.len();
        if self.0.len() < end {
            self.0.resize(end, 0);
        }
        self.0[offset as usize..end].copy_from_slice(page);
        true
    }

    fn read(&mut self, offset: u32, bytes: &mut [u8]) -> bool {
        let start = offset as usize;
        match self.0.get(start..start + bytes.len()) {
            Some(held) => {
                bytes.copy_from_slice(held);
                true
            }
            None => false,
        }
    }
}

/// Each input is one packet of an update. A board takes these from a sender it already trusts,
/// but the sender cannot be told from anyone copying its session, since pieces are not signed.
fn start_update() -> Parse {
    let start = || Update::begin(SESSION, IMAGE_SIZE, [1; 32], CAPACITY).ok();
    let mut update = start();
    let mut slot = Slot::default();
    let mut count = 0u32;
    Box::new(move |bytes| {
        let Some(running) = update.as_mut() else {
            return;
        };
        crate::taken();
        let (head, data) = bytes.split_at(bytes.len().min(4));
        let mut index = [0; 4];
        index[..head.len()].copy_from_slice(head);
        let index = u32::from_le_bytes(index);
        // Most pieces should land inside the image, so half of them are brought in.
        let index = if index & 1 == 0 { index >> 28 } else { index };
        running.piece(index, data.get(..data.len().min(PAGE)).unwrap_or(data));
        std::hint::black_box(running.missing());
        let _ = running.write_next(&mut slot);
        count += 1;
        if running.all_here() || count.is_multiple_of(64) {
            std::hint::black_box(running.verify(&mut slot).is_ok());
            update = start();
        }
    })
}

fn missing_lists() -> Vec<Vec<u8>> {
    let lists = [
        (0u32, vec![(0u32, 1u32)]),
        (100, vec![(1, 2), (9, 1)]),
        (0, vec![(0, 0)]),
        (0, vec![(u32::MAX, 1), (u32::MAX - 1, u32::MAX)]),
    ];
    lists.iter().filter_map(|list| postcard::to_allocvec(list).ok()).collect()
}

fn start_sending() -> Parse {
    let mut sending = Sending::new(SESSION, IMAGE_SIZE);
    Box::new(move |bytes| {
        if let Ok((done, runs)) = postcard::from_bytes::<(u32, Vec<(u32, u32)>)>(bytes) {
            crate::taken();
            sending.heard(done, runs);
        }
        for _ in 0..8 {
            std::hint::black_box(sending.next_piece());
        }
        std::hint::black_box(sending.must_ask());
    })
}

fn start_fleet() -> Parse {
    let folder = std::env::temp_dir().join(format!("olc-fuzz-fleet-{}", std::process::id()));
    let made = std::fs::create_dir_all(&folder).map_err(|error| error.to_string());
    let key = made.and_then(|()| ControlKey::load_or_create(&folder));
    let fleet = key.and_then(|key| Fleet::new(key, |peer| (peer >> 24) as usize));
    let mut fleet = fleet.expect("a fresh control key");
    Box::new(move |bytes| {
        let peer = u32::from(bytes.first().copied().unwrap_or(0));
        if Packet::decode(bytes).is_some() {
            crate::taken();
        }
        fleet.hear(peer, bytes);
        while fleet.take_outgoing().is_some() {}
        if !fleet.boards().is_empty() {
            crate::deep();
        }
    })
}

fn status_lines() -> Vec<Vec<u8>> {
    let line = "height=4 tip=ab work=9 peers=2 balance=1 pool=0 hashrate=10 limit=1000 \
                chosen=true mining=true offline=false address=aa view=coin smooth=10 reorg=3/1";
    vec![line.as_bytes().to_vec()]
}

fn read_status(bytes: &[u8]) {
    let text = String::from_utf8_lossy(bytes);
    if let Some(status) = Status::parse(&text) {
        crate::taken();
        std::hint::black_box(status.restore_lines());
        let mut state: BoardState = status.state.clone();
        state.after(&text);
    }
}

pub fn targets() -> Vec<Target> {
    vec![
        Target { name: "control.packet", seeds: packets, start: || Box::new(read_packet) },
        Target { name: "control.router", seeds: packets, start: start_router },
        Target { name: "control.gate", seeds: signed_orders, start: start_gate },
        Target { name: "control.gate_text", seeds: gate_lines, start: || Box::new(read_gate_text) },
        Target { name: "control.update", seeds: Vec::new, start: start_update },
        Target { name: "control.sending", seeds: missing_lists, start: start_sending },
        Target { name: "control.fleet", seeds: packets, start: start_fleet },
        Target { name: "board.status", seeds: status_lines, start: || Box::new(read_status) },
    ]
}
