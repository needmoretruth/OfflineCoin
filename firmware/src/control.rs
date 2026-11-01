//! The board's side of `olc-control`: hears packets on any link, passes on what is not for it,
//! and obeys what the gate lets through.
use crate::node::{hex, Node};
use crate::store::{Identity, Store};
use alloc::format;
use alloc::string::{String, ToString};
use core::sync::atomic::Ordering;
use esp_println::println;
use olc_control::{
    Action, Beacon, BoardId, Body, Forward, Gate, ImageSlot, Packet, Read, Refusal, Router, Signed,
    Update,
};
use olc_node::{Message, Outgoing};

const BEACON_EVERY_MS: u64 = 4_000;
/// How long printed lines keep going to whoever gave the last order.
const LISTEN_MS: u64 = 60_000;
/// Lets the last packet leave before the restart.
const FAREWELL_MS: u64 = 200;
const MAX_NAME: usize = 32;

/// Set by the `OLC_BUILD` variable at build time, to tell two builds apart.
pub const BUILD: &str = match option_env!("OLC_BUILD") {
    Some(build) => build,
    None => "dev",
};

pub struct Control {
    pub router: Router,
    pub gate: Gate,
    pub identity: Identity,
    pub slot: u8,
    update: Option<(Update, u32)>,
    listener: Option<(BoardId, u64)>,
    /// Who gets the answer to the question being handled.
    reader: Option<BoardId>,
    refused: u32,
    beacon_ms: u64,
}

impl Control {
    pub fn new(me: BoardId, first_id: u32, identity: Identity, counter: u64, slot: u8) -> Self {
        Self {
            router: Router::new(me, first_id),
            gate: Gate::new(me, identity.key, counter),
            identity,
            slot,
            update: None,
            listener: None,
            reader: None,
            refused: 0,
            beacon_ms: 0,
        }
    }
}

struct Spare<'a> {
    store: &'a mut Store,
    start: u32,
    size: u32,
}

impl ImageSlot for Spare<'_> {
    fn capacity(&self) -> u32 {
        self.size
    }

    fn write_page(&mut self, offset: u32, page: &[u8]) -> bool {
        self.store.write_page(self.start + offset, page)
    }

    fn read(&mut self, offset: u32, bytes: &mut [u8]) -> bool {
        self.store.read_flash(self.start + offset, bytes)
    }
}

impl Node {
    /// Handles what the links heard and says what this board shows, every few seconds.
    pub fn serve_control(&mut self, now: u64) {
        while let Some((peer, bytes)) = self.runner.take_control() {
            let Some(mut packet) = Packet::decode(&bytes) else {
                continue;
            };
            let verdict = self.control.router.incoming(peer, &packet);
            if verdict.forward != Forward::Nowhere {
                packet.hops += 1;
                self.emit(verdict.forward, &packet);
            }
            if verdict.for_me {
                self.obey(packet.from, packet.body, now);
            }
        }
        if now >= self.control.beacon_ms {
            self.control.beacon_ms = now + BEACON_EVERY_MS;
            let body = Body::Beacon(self.beacon());
            self.start_packet(None, body);
        }
    }

    /// Prints a line, and sends it to whoever is running this board from afar.
    pub fn say(&mut self, text: String) {
        println!("{text}");
        let listening = self
            .control
            .listener
            .filter(|(_, until)| crate::node::device_ms() < *until);
        if let Some(to) = listening.map(|(to, _)| to).or(self.control.reader) {
            self.start_packet(Some(to), Body::Line(text));
        }
    }

    pub fn trust(&mut self, key: olc_crypto::PublicKey) {
        self.control.identity.key = Some(key);
        self.store.save_identity(&self.control.identity);
        let me = self.control.router.me();
        self.control.gate = Gate::new(me, Some(key), self.control.gate.counter());
    }

    pub fn rename(&mut self, name: &str) {
        let name = name.char_indices().take_while(|(at, _)| *at < MAX_NAME);
        self.control.identity.name = name.map(|(_, letter)| letter).collect();
        self.store.save_identity(&self.control.identity);
    }

    fn beacon(&self) -> Beacon {
        let status = self.runner.engine.status();
        Beacon {
            name: self.control.identity.name.clone(),
            address: self.runner.engine.address(),
            height: status.height,
            tip: status.tip,
            balance: status.balance,
            hashrate: self.rate,
            limit: self.limit,
            peers: status.peers as u16,
            pool: status.pool as u16,
            mining: self.mining,
            offline: self.runner.offline,
            view: self.view.name().to_string(),
            language: self.language.name().to_string(),
            build: BUILD.to_string(),
            slot: self.control.slot,
            counter: self.control.gate.counter(),
            trusts_a_key: self.control.gate.has_key(),
            refused: self.control.refused,
        }
    }

    fn start_packet(&mut self, to: Option<BoardId>, body: Body) {
        let router = &mut self.control.router;
        let packet = Packet {
            from: router.me(),
            to,
            id: router.next_id(),
            hops: 0,
            body,
        };
        self.emit(self.control.router.outgoing(to), &packet);
    }

    fn emit(&mut self, forward: Forward, packet: &Packet) {
        let message = Message::Control(packet.encode());
        self.runner.send_now(match forward {
            Forward::Nowhere => return,
            Forward::Peer(peer) => Outgoing::To(peer, message),
            Forward::All => Outgoing::All {
                except: None,
                message,
            },
            Forward::AllExcept(peer) => Outgoing::All {
                except: Some(peer),
                message,
            },
        });
    }

    fn obey(&mut self, from: BoardId, body: Body, now: u64) {
        match body {
            Body::Read(read) => {
                self.control.reader = Some(from);
                match read {
                    Read::Status => self.print_status(),
                    Read::Tx(id) => self.print_tx(&hex(&id)),
                }
                self.control.reader = None;
            }
            Body::Order(signed) => self.take_order(from, &signed, now),
            Body::Chunk {
                session,
                offset,
                data,
            } => self.take_chunk(from, session, offset, &data),
            _ => {}
        }
    }

    fn take_order(&mut self, from: BoardId, signed: &Signed, now: u64) {
        match self.control.gate.admit(signed) {
            Ok(action) => {
                if !self.store.save_counter(self.control.gate.counter()) {
                    return self.refuse(from, Refusal::FlashFailed);
                }
                self.control.listener = Some((from, now + LISTEN_MS));
                match action {
                    Action::Line(line) => {
                        self.run_command(&line);
                        self.say(format!("done {}", signed.order.counter));
                        // The page shows the change at once instead of at the next beacon.
                        self.control.beacon_ms = 0;
                    }
                    Action::Update {
                        session,
                        size,
                        image,
                    } => self.start_update(from, session, size, image),
                }
            }
            Err(reason) => self.refuse(from, reason),
        }
    }

    fn refuse(&mut self, from: BoardId, reason: Refusal) {
        self.control.refused += 1;
        println!("refused {reason:?}");
        self.start_packet(Some(from), Body::Refused(reason));
    }

    fn start_update(&mut self, from: BoardId, session: u32, size: u32, image: olc_crypto::Hash) {
        let Some((start, capacity)) = self.store.spare_slot() else {
            return self.refuse(from, Refusal::FlashFailed);
        };
        match Update::begin(session, size, image, capacity) {
            Ok(update) => {
                self.control.update = Some((update, start));
                self.say(format!("update {size} bytes"));
                self.start_packet(Some(from), Body::Progress { session, next: 0 });
            }
            Err(reason) => self.refuse(from, reason),
        }
    }

    fn take_chunk(&mut self, from: BoardId, session: u32, offset: u32, data: &[u8]) {
        let Some((mut update, start)) = self.control.update.take() else {
            return self.refuse(from, Refusal::NoUpdateRunning);
        };
        if update.session() != session {
            self.control.update = Some((update, start));
            return self.refuse(from, Refusal::NoUpdateRunning);
        }
        let mut slot = Spare {
            store: &mut self.store,
            start,
            size: 0,
        };
        if let Err(reason) = update.chunk(&mut slot, offset, data) {
            return self.refuse(from, reason);
        }
        let done = update.done();
        let checked = done.then(|| update.verify(&mut slot));
        let next = update.next();
        match checked {
            None => self.control.update = Some((update, start)),
            Some(Err(reason)) => return self.refuse(from, reason),
            Some(Ok(())) => {}
        }
        self.start_packet(Some(from), Body::Progress { session, next });
        if done {
            self.switch_build(from);
        }
    }

    /// The whole image is in flash and matches its signature. Only now does the board change.
    fn switch_build(&mut self, from: BoardId) {
        if !self.store.boot_spare_slot() {
            return self.refuse(from, Refusal::FlashFailed);
        }
        self.say("update verified, restarting".to_string());
        let until = crate::node::device_ms() + FAREWELL_MS;
        while crate::node::device_ms() < until {}
        esp_hal::system::software_reset();
    }
}

/// True while a line is arriving on the cable, so mining should wait for it.
pub fn cable_is_busy(now: u64) -> bool {
    let seen = crate::usb::LINE_IN_PROGRESS_MS.load(Ordering::Relaxed);
    (now as u32).wrapping_sub(seen) < crate::node::CABLE_QUIET_MS
}

/// Decodes a control key from the `trust` line.
pub fn parse_key(text: &str) -> Option<olc_crypto::PublicKey> {
    let mut key = [0u8; 33];
    if text.len() != key.len() * 2 || !text.is_ascii() {
        return None;
    }
    for (index, byte) in key.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16).ok()?;
    }
    Some(key)
}
