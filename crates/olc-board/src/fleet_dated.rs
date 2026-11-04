//! The date for the boards, which have no clock of it. Every board on this computer's cables that
//! trusts its control key gets the computer's time, signed, the moment it comes on and every two
//! minutes after; each passes it one hop on by radio to the boards heard only through it. See
//! `olc_control::dated`.
use super::{Fleet, LATELY};
use olc_control::dated::{sign_dated, DATED_EVERY_MS};
use olc_control::{Body, BoardId};
use olc_core::timing::DATED_CLOCK_FROM_MS;
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

/// When each board on a cable was last given the date. A board missing here is new on its cable,
/// or came back after a silence such as a restart, and gets it at once.
#[derive(Default)]
pub struct Dating {
    pub(super) given: BTreeMap<BoardId, Instant>,
}

impl Fleet {
    /// Signs `unix_ms`, this computer's time, for every board on a cable that is due. A clock that
    /// reads before 2020 has no date to give.
    pub fn give_dates(&mut self, unix_ms: u64) {
        if unix_ms < DATED_CLOCK_FROM_MS {
            return;
        }
        let every = Duration::from_millis(DATED_EVERY_MS);
        let on_cable: Vec<BoardId> = self
            .boards
            .iter()
            .filter(|(_, known)| known.beacon.trusts_a_key)
            .filter(|(_, known)| known.direct.is_some_and(|(when, _)| when.elapsed() < LATELY))
            .map(|(id, _)| *id)
            .collect();
        self.dating.given.retain(|id, _| on_cable.contains(id));
        for id in on_cable {
            let due = self.dating.given.get(&id).is_none_or(|given| given.elapsed() >= every);
            if !due {
                continue;
            }
            let Some(dated) = sign_dated(self.key.secret(), unix_ms) else {
                return;
            };
            self.start_packet(id, Body::Dated(dated));
            self.dating.given.insert(id, Instant::now());
        }
    }
}
