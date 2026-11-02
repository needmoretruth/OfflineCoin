//! A board sends the build it runs to another board by radio. The computer's cable is the slow
//! part of the way, and any build that may go out has already run on a board with a cable, so
//! that board can hand it on from its own flash.
use crate::node::{device_ms, Node};
use alloc::vec;
use alloc::vec::Vec;
use olc_control::sending::Sending;
use olc_control::{BoardId, Body, Refusal, PAGE};
use olc_crypto::{Hash, StreamHash};

const PIECES_PER_TURN: u32 = 4;
const ASK_AGAIN_MS: u64 = 300;
const GIVE_UP_MS: u64 = 20_000;

pub struct Serving {
    target: BoardId,
    start: u32,
    sending: Sending,
    ask_ms: u64,
    heard_ms: u64,
}

impl Serving {
    pub fn moving(&self) -> bool {
        self.sending.moving()
    }
}

impl Node {
    /// Starts sending this board's own image, if it is the one the order names.
    pub fn start_serving(
        &mut self,
        from: BoardId,
        target: BoardId,
        session: u32,
        size: u32,
        image: Hash,
    ) {
        let Some(start) = self.store.running_start() else {
            return self.refuse(from, Refusal::FlashFailed);
        };
        if !self.slot_holds(start, size, &image) {
            return self.refuse(from, Refusal::BadImage);
        }
        let now = device_ms();
        self.control.serving = Some(Serving {
            target,
            start,
            sending: Sending::new(session, size),
            ask_ms: now,
            heard_ms: now,
        });
        self.start_packet(Some(from), Body::Progress { session, next: 0 });
    }

    fn slot_holds(&mut self, start: u32, size: u32, image: &Hash) -> bool {
        let mut hasher = StreamHash::default();
        let mut page = vec![0u8; PAGE];
        let mut offset = 0;
        while offset < size {
            let count = (size - offset).min(PAGE as u32) as usize;
            if !self.store.read_flash(start + offset, &mut page[..count]) {
                return false;
            }
            hasher.update(&page[..count]);
            offset += count as u32;
        }
        hasher.finish() == *image
    }

    /// The other board's word on what it lacks.
    pub fn hear_missing(&mut self, session: u32, done: u32, runs: Vec<(u32, u32)>) {
        let Some(serving) = &mut self.control.serving else {
            return;
        };
        if serving.sending.session() != session {
            return;
        }
        serving.heard_ms = device_ms();
        if serving.sending.heard(done, runs) {
            self.control.serving = None;
        }
    }

    pub fn serve_turn(&mut self, now: u64) {
        let Some(mut serving) = self.control.serving.take() else {
            return;
        };
        if now > serving.heard_ms + GIVE_UP_MS {
            return;
        }
        for _ in 0..PIECES_PER_TURN {
            let Some((index, offset, length)) = serving.sending.next_piece() else {
                break;
            };
            self.send_piece(&serving, index, offset, length);
        }
        if serving.sending.must_ask() && now >= serving.ask_ms {
            serving.ask_ms = now + ASK_AGAIN_MS;
            let session = serving.sending.session();
            self.start_packet(Some(serving.target), Body::AskMissing { session });
        }
        self.control.serving = Some(serving);
    }

    fn send_piece(&mut self, serving: &Serving, index: u32, offset: u32, length: usize) {
        let mut data = vec![0u8; length];
        if self.store.read_flash(serving.start + offset, &mut data) {
            let body = Body::Piece {
                session: serving.sending.session(),
                index,
                data,
            };
            self.start_packet(Some(serving.target), body);
        }
    }
}
