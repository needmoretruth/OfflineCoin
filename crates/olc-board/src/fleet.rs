//! This computer's view of every board it can hear, and the packets it sends them.
use crate::control_key::ControlKey;
use crate::pins::Pins;
use olc_control::lend::{sign_lent, Address};
use olc_control::{
    sign, Action, Beacon, BoardId, Body, Forward, OldBeacon, Order, Packet, Read, Refusal, Router,
    Signed,
};
use std::collections::{BTreeMap, VecDeque};
use std::time::{Duration, Instant};

/// Four beacons in a row lost, plus some room for a board busy with flash.
const SILENT_AFTER: Duration = Duration::from_secs(25);
const DIRECT_FOR: Duration = Duration::from_secs(14);
/// A little longer than a board waits between two beacons.
const LATELY: Duration = Duration::from_secs(6);
const MAX_LINES: usize = 256;
/// Boards and sessions kept in mind. Packets on the air name any board they like, so one more
/// made-up name must not cost one more thing to keep. A real board comes back with its next beacon.
const MOST_BOARDS: usize = 256;

fn room_for<K: Ord, V>(list: &mut BTreeMap<K, V>, key: &K) {
    if list.len() >= MOST_BOARDS && !list.contains_key(key) {
        list.pop_first();
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Connection {
    /// On this computer's own link.
    Direct { link: usize },
    /// Reached by radio through another board.
    Through { board: Option<BoardId> },
}

#[derive(Clone, Debug)]
pub struct Seen {
    pub id: BoardId,
    pub beacon: Beacon,
    pub connection: Connection,
    pub online: bool,
    pub silent_for: Duration,
    /// Its build speaks a beacon format this program cannot check, so what it says is unproved.
    pub old_format: bool,
}

struct Known {
    beacon: Beacon,
    old_format: bool,
    heard: Instant,
    direct: Option<(Instant, usize)>,
    link: usize,
}

pub struct Fleet {
    key: ControlKey,
    router: Router,
    link_of: fn(u32) -> usize,
    boards: BTreeMap<BoardId, Known>,
    counters: BTreeMap<BoardId, u64>,
    lines: BTreeMap<BoardId, VecDeque<String>>,
    finished: BTreeMap<BoardId, u64>,
    refusals: BTreeMap<BoardId, VecDeque<Refusal>>,
    progress: BTreeMap<u32, u32>,
    missing: BTreeMap<u32, (u32, Vec<(u32, u32)>)>,
    outgoing: VecDeque<(Forward, Vec<u8>)>,
    pins: Pins,
    /// Headers boards found for lent work, by the address each board mines to.
    shares: BTreeMap<Address, VecDeque<Vec<u8>>>,
}

fn random_id() -> Result<(BoardId, u32), String> {
    let mut bytes = [0u8; 10];
    getrandom::fill(&mut bytes).map_err(|error| error.to_string())?;
    let mut id = [0u8; 6];
    id.copy_from_slice(&bytes[..6]);
    Ok((
        id,
        u32::from_le_bytes([bytes[6], bytes[7], bytes[8], bytes[9]]),
    ))
}

impl Fleet {
    /// `link_of` turns a peer id into the position of its link.
    pub fn new(key: ControlKey, link_of: fn(u32) -> usize) -> Result<Self, String> {
        let (id, first) = random_id()?;
        Ok(Self {
            key,
            router: Router::new(id, first),
            link_of,
            boards: BTreeMap::new(),
            counters: BTreeMap::new(),
            lines: BTreeMap::new(),
            finished: BTreeMap::new(),
            refusals: BTreeMap::new(),
            progress: BTreeMap::new(),
            missing: BTreeMap::new(),
            outgoing: VecDeque::new(),
            pins: Pins::default(),
            shares: BTreeMap::new(),
        })
    }

    /// Remembers across runs which board spoke with which address.
    pub fn keep_pins(&mut self, pins: Pins) {
        self.pins = pins;
    }

    pub fn public_key(&self) -> Result<olc_crypto::PublicKey, String> {
        self.key.public()
    }

    /// Takes in a packet a link heard.
    pub fn hear(&mut self, peer: u32, bytes: &[u8]) {
        let Some(mut packet) = Packet::decode(bytes) else {
            self.hear_old_board(peer, bytes);
            return;
        };
        if let Body::Beacon(signed) = &packet.body {
            // Hops count only what the board itself sent: zero means its own cable.
            let on_cable = packet.hops == 0;
            let address = signed.beacon.address;
            if !signed.is_from(&packet.from) || !self.pins.accept(packet.from, address, on_cable) {
                return;
            }
        }
        if let (Body::Beacon(_), Some(known)) = (&packet.body, self.boards.get_mut(&packet.from)) {
            // Copies count here: the first to arrive may be the one that took the long way.
            if packet.hops == 0 {
                known.direct = Some((Instant::now(), (self.link_of)(peer)));
            }
        }
        let verdict = self.router.incoming(peer, &packet);
        if verdict.forward != Forward::Nowhere {
            packet.hops += 1;
            self.outgoing.push_back((verdict.forward, packet.encode()));
            packet.hops -= 1;
        }
        if verdict.for_me {
            self.take_in(peer, packet);
        }
    }

    /// A board never disappears for being old. Unproved words stay away from any board that
    /// has proved itself: they can add a row, never change one.
    fn hear_old_board(&mut self, peer: u32, bytes: &[u8]) {
        let Some(old) = OldBeacon::decode(bytes) else {
            return;
        };
        let from = old.from;
        if self
            .boards
            .get(&from)
            .is_some_and(|known| !known.old_format)
        {
            return;
        }
        let link = (self.link_of)(peer);
        let direct = (old.hops == 0).then(|| (Instant::now(), link));
        let known = Known {
            beacon: old.beacon(),
            old_format: true,
            heard: Instant::now(),
            direct,
            link,
        };
        // Unproved words give way to each other, never to push out a board that proved itself.
        if self.boards.len() >= MOST_BOARDS && !self.boards.contains_key(&from) {
            let unproved = self.boards.iter().find(|(_, known)| known.old_format);
            let Some(oldest) = unproved.map(|(id, _)| *id) else {
                return;
            };
            self.boards.remove(&oldest);
        }
        self.boards.insert(from, known);
    }

    fn take_in(&mut self, peer: u32, packet: Packet) {
        let from = packet.from;
        match packet.body {
            Body::Beacon(signed) => {
                let beacon = signed.beacon;
                let link = (self.link_of)(peer);
                let direct = (packet.hops == 0).then(|| (Instant::now(), link));
                let old = self.boards.get(&from).and_then(|known| known.direct);
                let known = Known {
                    beacon,
                    old_format: false,
                    heard: Instant::now(),
                    direct: direct.or(old),
                    link,
                };
                room_for(&mut self.boards, &from);
                self.boards.insert(from, known);
            }
            Body::Line(line) => {
                if let Some(counter) = line.strip_prefix("done ") {
                    let counter = counter.parse().unwrap_or(0);
                    room_for(&mut self.finished, &from);
                    self.finished.insert(from, counter);
                }
                room_for(&mut self.lines, &from);
                let lines = self.lines.entry(from).or_default();
                if lines.len() >= MAX_LINES {
                    lines.pop_front();
                }
                lines.push_back(line);
            }
            Body::Progress { session, next } => {
                room_for(&mut self.progress, &session);
                self.progress.insert(session, next);
            }
            Body::Missing {
                session,
                done,
                runs,
            } => {
                room_for(&mut self.missing, &session);
                self.missing.insert(session, (done, runs));
            }
            Body::Refused(reason) => {
                room_for(&mut self.refusals, &from);
                let refusals = self.refusals.entry(from).or_default();
                if refusals.len() >= MAX_LINES {
                    refusals.pop_front();
                }
                refusals.push_back(reason);
            }
            Body::Shares { worker, headers } => {
                room_for(&mut self.shares, &worker);
                let kept = self.shares.entry(worker).or_default();
                for header in headers {
                    if kept.len() >= MAX_LINES {
                        kept.pop_front();
                    }
                    kept.push_back(header);
                }
            }
            _ => {}
        }
    }

    pub fn take_outgoing(&mut self) -> Option<(Forward, Vec<u8>)> {
        self.outgoing.pop_front()
    }

    fn start_packet(&mut self, to: BoardId, body: Body) {
        let packet = Packet {
            from: self.router.me(),
            to: Some(to),
            id: self.router.next_id(),
            hops: 0,
            body,
        };
        let forward = self.router.outgoing(Some(to));
        self.outgoing.push_back((forward, packet.encode()));
    }

    pub fn read(&mut self, board: BoardId, read: Read) {
        self.start_packet(board, Body::Read(read));
    }

    /// Signs an order with the next counter this board has not seen.
    pub fn sign(&mut self, board: BoardId, action: Action) -> Result<Signed, String> {
        let shown = self
            .boards
            .get(&board)
            .map_or(0, |known| known.beacon.counter);
        let counter = self.counters.get(&board).copied().unwrap_or(0).max(shown) + 1;
        self.counters.insert(board, counter);
        let order = Order {
            board,
            counter,
            action,
        };
        sign(self.key.secret(), order).ok_or_else(|| "The control key is bad".to_string())
    }

    pub fn send_signed(&mut self, board: BoardId, signed: Signed) {
        self.start_packet(board, Body::Order(signed));
    }

    /// Signs and sends an order once.
    pub fn order(&mut self, board: BoardId, action: Action) -> Result<(), String> {
        let signed = self.sign(board, action)?;
        self.send_signed(board, signed);
        Ok(())
    }

    /// Signs pool work for the board that mines to `worker` and sends it to `relay`, the board
    /// that hears it. `None` stops the work.
    pub fn lend(&mut self, relay: BoardId, worker: Address, work: Option<(Vec<u8>, u64)>) -> Result<(), String> {
        let lent = sign_lent(self.key.secret(), worker, work).ok_or("The control key is bad")?;
        self.start_packet(relay, Body::Lend(lent));
        Ok(())
    }

    /// The headers the board that mines to `worker` found for lent work since the last call.
    pub fn take_shares(&mut self, worker: &Address) -> Vec<Vec<u8>> {
        self.shares.remove(worker).map(Vec::from).unwrap_or_default()
    }

    /// Sends a packet exactly as given. Tests of the board's refusals need this.
    pub fn send_body(&mut self, board: BoardId, body: Body) {
        self.start_packet(board, body);
    }

    pub fn chunk(&mut self, board: BoardId, session: u32, offset: u32, data: Vec<u8>) {
        let body = Body::Chunk {
            session,
            offset,
            data,
        };
        self.start_packet(board, body);
    }

    pub fn take_line(&mut self, board: BoardId) -> Option<String> {
        self.lines.get_mut(&board)?.pop_front()
    }

    pub fn take_refusal(&mut self, board: BoardId) -> Option<Refusal> {
        self.refusals.get_mut(&board)?.pop_front()
    }

    pub fn has_finished(&self, board: BoardId, counter: u64) -> bool {
        self.finished
            .get(&board)
            .is_some_and(|done| *done >= counter)
    }

    /// The board's last word on what it still lacks, once.
    pub fn take_missing(&mut self, session: u32) -> Option<(u32, Vec<(u32, u32)>)> {
        self.missing.remove(&session)
    }

    pub fn progress(&self, session: u32) -> Option<u32> {
        self.progress.get(&session).copied()
    }

    fn owner_of_link(&self, link: usize) -> Option<BoardId> {
        self.boards
            .iter()
            .find(|(_, known)| {
                matches!(known.direct, Some((when, at))
                    if at == link && when.elapsed() < DIRECT_FOR)
            })
            .map(|(id, _)| *id)
    }

    /// Every board heard so far, in a steady order.
    pub fn boards(&self) -> Vec<Seen> {
        let view = |(id, known): (&BoardId, &Known)| {
            let direct = known.direct.filter(|(when, _)| when.elapsed() < DIRECT_FOR);
            Seen {
                id: *id,
                beacon: known.beacon.clone(),
                connection: match direct {
                    Some((_, link)) => Connection::Direct { link },
                    None => Connection::Through {
                        board: self.owner_of_link(known.link),
                    },
                },
                online: known.heard.elapsed() < SILENT_AFTER,
                silent_for: known.heard.elapsed(),
                old_format: known.old_format,
            }
        };
        self.boards.iter().map(view).collect()
    }

    /// Whether a board spoke on this link just now. A board that has restarted stays quiet on
    /// its cable until it hears from this side.
    pub fn heard_lately(&self, link: usize) -> bool {
        self.boards.values().any(|known| {
            matches!(known.direct, Some((when, at)) if at == link && when.elapsed() < LATELY)
        })
    }

    /// Whether the board on this link is one the app can already order about.
    pub fn serves(&self, link: usize) -> bool {
        self.owner_of_link(link)
            .and_then(|id| self.boards.get(&id))
            .is_some_and(|known| known.beacon.trusts_a_key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control_key::ControlKey;
    use olc_control::sign_beacon;
    use olc_crypto::Signer;

    const LINK_SHIFT: u32 = 24;
    const BOARD: BoardId = [1, 2, 3, 4, 5, 6];
    const CABLE: u32 = 1 << LINK_SHIFT;
    const SECRET: [u8; 32] = [3; 32];

    fn link_of(peer: u32) -> usize {
        (peer >> LINK_SHIFT) as usize
    }

    fn fleet(name: &str) -> Fleet {
        let path = std::env::temp_dir().join(format!("olc-fleet-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap();
        let key = ControlKey::load_or_create(&path).unwrap();
        std::fs::remove_dir_all(&path).unwrap();
        Fleet::new(key, link_of).unwrap()
    }

    fn beacon(id: u32, hops: u8, from: BoardId) -> Vec<u8> {
        let signer = Signer::new(&SECRET).unwrap();
        let beacon = Beacon {
            name: "one".into(),
            address: olc_crypto::address_of(signer.public_key()),
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
        Packet {
            from,
            to: None,
            id,
            hops,
            body: Body::Beacon(sign_beacon(&signer, &from, beacon).unwrap()),
        }
        .encode()
    }

    /// The bytes a build from before `smooth` and `genesis` sends: the fields it had, then a
    /// signature in a place today's format reads as something else.
    fn old_frame(from: BoardId, hops: u8) -> Vec<u8> {
        let mut frame = OldBeacon::decode(&beacon(1, hops, from)).unwrap().encode();
        frame.extend_from_slice(&[0, 0, 0]);
        frame
    }

    #[test]
    fn made_up_boards_stay_on_a_short_list() {
        let mut fleet = fleet("made-up");
        for number in 0..2_000u32 {
            let mut from = [9; 6];
            from[..4].copy_from_slice(&number.to_le_bytes());
            let session = number;
            let bodies = [
                Body::Line(format!("done {number}")),
                Body::Refused(Refusal::Replayed),
                Body::Progress { session, next: 1 },
                Body::Missing { session, done: 0, runs: Vec::new() },
            ];
            for (at, body) in bodies.into_iter().enumerate() {
                let id = number * 4 + at as u32;
                fleet.hear(1, &Packet { from, to: None, id, hops: 0, body }.encode());
            }
            fleet.hear(1, &old_frame(from, 1));
        }
        assert!(fleet.boards.len() <= MOST_BOARDS, "{} boards", fleet.boards.len());
        assert!(fleet.lines.len() <= MOST_BOARDS, "{} line lists", fleet.lines.len());
        assert!(fleet.finished.len() <= MOST_BOARDS);
        assert!(fleet.refusals.len() <= MOST_BOARDS);
        assert!(fleet.progress.len() <= MOST_BOARDS);
        assert!(fleet.missing.len() <= MOST_BOARDS);

        let from = [8; 6];
        for id in 0..2_000 {
            let body = Body::Refused(Refusal::Replayed);
            fleet.hear(1, &Packet { from, to: None, id, hops: 0, body }.encode());
        }
        assert!(fleet.refusals[&from].len() <= MAX_LINES);
    }

    #[test]
    fn made_up_old_boards_cannot_push_out_a_proved_one() {
        let mut fleet = fleet("pushed");
        fleet.hear(1, &beacon(1, 1, BOARD));
        for number in 0..2_000u32 {
            let mut from = [7; 6];
            from[..4].copy_from_slice(&number.to_le_bytes());
            fleet.hear(1, &old_frame(from, 1));
        }
        assert!(fleet.boards.contains_key(&BOARD));
        assert!(!fleet.boards[&BOARD].old_format);
    }

    #[test]
    fn a_board_too_old_to_read_still_shows() {
        let mut fleet = fleet("old-board");
        fleet.hear(1, &old_frame(BOARD, 1));
        let boards = fleet.boards();
        assert_eq!(boards.len(), 1);
        assert!(boards[0].old_format);
        assert_eq!(boards[0].beacon.name, "one");
        assert_eq!(boards[0].beacon.build, "x");
        assert_eq!(boards[0].beacon.height, 9);
    }

    #[test]
    fn old_words_cannot_change_a_board_that_proved_itself() {
        let mut fleet = fleet("old-words");
        fleet.hear(1, &beacon(1, 1, BOARD));
        let mut frame = OldBeacon::decode(&beacon(2, 1, BOARD)).unwrap();
        frame.name = "fake".into();
        fleet.hear(1, &frame.encode());
        let boards = fleet.boards();
        assert_eq!(boards[0].beacon.name, "one");
        assert!(!boards[0].old_format);
    }

    #[test]
    fn a_beacon_nobody_signed_shows_no_board() {
        let mut fleet = fleet("forged");
        let mut packet = Packet::decode(&beacon(1, 1, BOARD)).unwrap();
        if let Body::Beacon(signed) = &mut packet.body {
            signed.beacon.height = 10;
        }
        fleet.hear(CABLE, &packet.encode());
        assert!(fleet.boards().is_empty());
    }

    #[test]
    fn another_key_cannot_take_over_a_board_by_radio() {
        let mut fleet = fleet("taken");
        fleet.hear(CABLE, &beacon(1, 1, BOARD));
        let other = Signer::new(&[9; 32]).unwrap();
        let forged = Beacon {
            address: olc_crypto::address_of(other.public_key()),
            height: 99,
            ..Beacon::default()
        };
        let forged = Packet {
            from: BOARD,
            to: None,
            id: 2,
            hops: 1,
            body: Body::Beacon(sign_beacon(&other, &BOARD, forged).unwrap()),
        };
        fleet.hear(CABLE, &forged.encode());
        assert_eq!(fleet.boards()[0].beacon.height, 9);
    }

    #[test]
    fn a_board_on_the_cable_is_direct_and_one_behind_it_is_not() {
        let mut fleet = fleet("direct");
        fleet.hear(CABLE, &beacon(1, 0, BOARD));
        let far = [9; 6];
        fleet.hear(CABLE, &beacon(2, 1, far));
        let boards = fleet.boards();
        let find = |id| boards.iter().find(|seen| seen.id == id).unwrap();
        assert_eq!(find(BOARD).connection, Connection::Direct { link: 1 });
        assert_eq!(
            find(far).connection,
            Connection::Through { board: Some(BOARD) }
        );
    }

    #[test]
    fn a_direct_copy_counts_even_after_the_radio_copy_came_first() {
        let mut fleet = fleet("copies");
        fleet.hear(CABLE, &beacon(1, 0, BOARD));
        fleet.hear(CABLE, &beacon(2, 1, BOARD));
        fleet.hear(CABLE, &beacon(2, 0, BOARD));
        let boards = fleet.boards();
        assert_eq!(boards[0].connection, Connection::Direct { link: 1 });
    }

    #[test]
    fn orders_count_up_from_what_the_board_showed() {
        let mut fleet = fleet("counter");
        fleet.hear(CABLE, &beacon(1, 0, BOARD));
        let first = fleet.sign(BOARD, Action::Line("status".into())).unwrap();
        let second = fleet.sign(BOARD, Action::Line("status".into())).unwrap();
        assert_eq!(first.order.counter, 42);
        assert_eq!(second.order.counter, 43);
    }
}
