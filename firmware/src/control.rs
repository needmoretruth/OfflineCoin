//! The board's side of `olc-control`: hears packets on any link, passes on what is not for it,
//! and obeys what the gate lets through.
use crate::node::{hex, Node};
use crate::node::{LINK_RADIO, LINK_USB};
use crate::store::{Identity, Store};
use crate::views::Note;
use crate::wifi;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::sync::atomic::Ordering;
use esp_println::println;
use olc_control::{
    sign_beacon, Action, Beacon, BoardId, Body, Forward, Gate, ImageSlot, OldBeacon, Packet, Read,
    Refusal, Router, Signed, Update,
};
use olc_crypto::Signer;
use olc_logo::{MAX_FPS, MIN_FPS};
use olc_node::{Card, Message, Outgoing};

const BEACON_EVERY_MS: u64 = 4_000;
const FIRST_BEACON_AFTER_MS: u64 = 1_200;
/// How often a board with a computer on its cable asks the boards around what they are.
const ASK_AROUND_EVERY_MS: u64 = 30_000;
const DEVICE: &str = "esp32-s3";
const CHUNK_PAUSE_MS: u64 = 1_000;
const UPDATE_GONE_MS: u64 = 15_000;
/// How long printed lines keep going to whoever gave the last order.
const LISTEN_MS: u64 = 60_000;
/// Lets the last packet leave before the restart.
const FAREWELL_MS: u64 = 200;
const MAX_NAME: usize = 32;

/// From `build-id`, which goes up with every firmware change that should reach boards.
/// `OLC_BUILD` overrides it for a build made by hand.
pub const BUILD: &str = match option_env!("OLC_BUILD") {
    Some(build) => build,
    None => include_str!("../build-id").trim_ascii(),
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
    ask_around_ms: u64,
    /// Signs beacons. It is the wallet's key: the address is what the app knows a board by.
    signer: Signer,
    /// When the last piece of an image came.
    chunk_ms: u64,
}

impl Control {
    /// How far an image that is still arriving has come.
    pub fn update_percent(&self, now: u64) -> Option<u8> {
        let (update, _) = self.update.as_ref()?;
        (now < self.chunk_ms + UPDATE_GONE_MS).then(|| update.percent())
    }

    /// True while someone with the key listens to this board.
    pub fn is_managed(&self, now: u64) -> bool {
        self.listener.is_some_and(|(_, until)| now < until)
    }

    /// A piece of an image is several radio frames and the radio holds only a few, so mining must
    /// not sit on them.
    pub fn taking_an_image(&self, now: u64) -> bool {
        self.update.is_some() && now < self.chunk_ms + CHUNK_PAUSE_MS
    }

    pub fn new(
        me: BoardId,
        first_id: u32,
        identity: Identity,
        counter: u64,
        slot: u8,
        signer: Signer,
    ) -> Self {
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
            ask_around_ms: 0,
            signer,
            chunk_ms: 0,
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
                self.pass_on_old_beacon(peer, &bytes);
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
            self.runner.card = self.card();
            let me = self.control.router.me();
            if let Some(signed) = sign_beacon(&self.control.signer, &me, self.beacon()) {
                self.start_packet(None, Body::Beacon(signed));
            }
        }
        self.pass_hellos(now);
    }

    /// What this board says when asked. Public facts only.
    fn card(&self) -> Card {
        Card {
            name: self.control.identity.name.clone(),
            device: DEVICE.into(),
            build: BUILD.into(),
            port: wifi::PORT,
            mining: self.mining,
            hashrate: u64::from(self.rate),
            known: Vec::new(),
        }
    }

    /// A board on the air alone can only tell the computer through a board on a cable.
    fn pass_hellos(&mut self, now: u64) {
        let cabled = crate::usb::host_spoke();
        if cabled && now >= self.control.ask_around_ms {
            self.control.ask_around_ms = now + ASK_AROUND_EVERY_MS;
            self.runner.ask_who_on(LINK_RADIO);
        }
        while let Some((_, hello)) = self.runner.take_hello() {
            if cabled {
                self.runner.pass_hello(LINK_USB, hello);
            }
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
            smooth: self.smooth,
            smooth_range: (MIN_FPS, MAX_FPS),
            genesis: self.runner.engine.chain().genesis_id(),
            work: status.work,
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

    /// A build too old to read still reaches the app, which shows that it is there.
    fn pass_on_old_beacon(&mut self, peer: u32, bytes: &[u8]) {
        let Some(mut old) = OldBeacon::decode(bytes) else {
            return;
        };
        let forward = self.control.router.incoming(peer, &old.route()).forward;
        old.hops += 1;
        self.send_bytes(forward, old.encode());
    }

    /// Boot time is no measure of hashing, so the first beacon waits for a window of its own.
    pub fn first_window(&mut self, now: u64) {
        self.window_ms = now;
        self.control.beacon_ms = now + FIRST_BEACON_AFTER_MS;
    }

    fn emit(&mut self, forward: Forward, packet: &Packet) {
        self.send_bytes(forward, packet.encode());
    }

    fn send_bytes(&mut self, forward: Forward, bytes: Vec<u8>) {
        let message = Message::Control(bytes);
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
                        let word = order_word(&line);
                        if !matches!(word, "status" | "tx" | "pool" | "boots") {
                            self.note(now, Note::Order(word));
                        }
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
        self.control.chunk_ms = crate::node::device_ms();
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

/// The order's first word, with a lifetime the panel can keep.
fn order_word(line: &str) -> &'static str {
    let word = line.split_whitespace().next().unwrap_or("");
    let known = olc_control::REMOTE_WORDS
        .iter()
        .find(|known| **known == word);
    known.copied().unwrap_or("?")
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
