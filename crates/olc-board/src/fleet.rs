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
    /// When lent work for each worker was last passed on, and last taken.
    lent_heard: BTreeMap<Address, LentHeard>,
}

/// What came back for lent work: when the board on the cable last said it passed work on to the
/// worker, and when the worker last said it took work. Neither is signed, so they only explain
/// silence; shares are checked by the pool.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LentHeard {
    pub passed: Option<Instant>,
    pub taken: Option<Instant>,
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
            lent_heard: BTreeMap::new(),
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
            Body::Passed { worker } => {
                room_for(&mut self.lent_heard, &worker);
                self.lent_heard.entry(worker).or_default().passed = Some(Instant::now());
            }
            Body::Shares { worker, headers } => {
                room_for(&mut self.lent_heard, &worker);
                self.lent_heard.entry(worker).or_default().taken = Some(Instant::now());
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

    pub fn lent_heard(&self, worker: &Address) -> LentHeard {
        self.lent_heard.get(worker).copied().unwrap_or_default()
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
#[path = "fleet_tests.rs"]
mod tests;
