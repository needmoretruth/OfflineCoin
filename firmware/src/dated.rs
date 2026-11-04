//! The date, which a board has no clock for. The app that controls it signs its own time and sends
//! it down the cable; the board on the cable passes it one hop on by radio. The board keeps it in
//! flash, so after a restart it starts from the last date it knew rather than from its chain's tip.
//! What to take and when to save is decided in `olc_control::dated`, where the computer's tests
//! run the same rules.
use crate::flash;
use crate::node::Node;
use crate::store::Nor;
use olc_control::dated::{handle, read_record, record, Handling};
use olc_control::wear::Ring;
use olc_control::{Body, Packet};
use olc_node::{Message, Outgoing};

/// Two sectors after the node's choices in the `chain` partition, which nothing else uses.
const DATED: Ring = Ring {
    start: 0xBF_5000,
    sectors: 2,
    slot: 16,
};

/// The last date this board saved, if a whole record is there.
pub fn saved() -> Option<u64> {
    DATED.newest_first(&mut Nor, 2).into_iter().find_map(|at| {
        let mut bytes = [0u8; 16];
        flash::read(at, &mut bytes).then(|| read_record(&bytes)).flatten()
    })
}

fn save(unix_ms: u64) {
    let at = DATED.next(&mut Nor);
    if flash::program(at, &record(unix_ms)) {
        DATED.written(&mut Nor, at);
    }
}

impl Node {
    /// A signed time that reached this board: taken as its clock, and passed on once by the board
    /// on the cable.
    pub fn dated(&mut self, packet: &Packet, now: u64) {
        let Handling::Take { unix_ms, pass } = handle(packet, &self.control.gate, self.control.date_passed) else {
            return;
        };
        // A clock already past the time, which a block stamped ahead can set, stays where it is.
        self.runner.engine.take_date(unix_ms, now);
        self.control.keeping.learned(now, false);
        if let Some(pass) = pass {
            self.control.date_passed = unix_ms;
            let message = Message::Control(pass.encode());
            self.runner.send_now(Outgoing::All { except: None, message });
        }
    }

    /// Writes the date this board holds now and then, so a restart starts near it.
    pub fn keep_date(&mut self, now: u64) {
        if self.control.keeping.due(now) {
            save(self.runner.engine.clock_at(now));
        }
    }
}

/// Whether `body` is a time for [`Node::dated`].
pub fn is_dated(body: &Body) -> bool {
    matches!(body, Body::Dated(_))
}
