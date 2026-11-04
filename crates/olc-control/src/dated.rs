//! The date, from the computer that runs the boards. A board has no clock of the date: left to
//! itself it counts from its chain's tip, and a fresh chain's tip is the coin's genesis, so every
//! block it made would read as old as the coin. The app signs its own time with its control key
//! and sends it to each board on its cables, and the board on the cable passes it one hop on by
//! radio to every board that hears it.
//!
//! Nothing counts these, so nothing is written to flash for each: a board takes a time only when
//! it moves its clock forward, which an old copy sent again never does, and it passes on only a
//! time later than any it passed before.
use crate::gate::Gate;
use crate::packet::{Body, Packet};
use crate::router::MAX_HOPS;
use alloc::boxed::Box;
use alloc::vec::Vec;
use olc_crypto::{hash, Hash};
use serde::{Deserialize, Serialize};

const DOMAIN: &[u8] = b"OfflineCoin dated time 1\n";
/// The app gives every board it hears the date this often, and at once when one comes on.
pub const DATED_EVERY_MS: u64 = 120_000;
/// A board writes the date it holds to flash this often, so a restart starts near the date
/// rather than at its chain's tip, and a quick restart stays within the minute and a half after
/// which a block counts as late. One 16-byte record a minute in a ring of two sectors, 512
/// records a round: each sector is erased once a round, about every eight and a half hours,
/// and its 100,000 erases last some 97 years. Writing the record takes microseconds; the erase
/// holds the board for a moment three times a day.
pub const SAVE_EVERY_MS: u64 = 60_000;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dated {
    /// Milliseconds since 1970 by the computer's clock when it signed.
    pub unix_ms: u64,
    pub signature: Vec<u8>,
}

fn digest(unix_ms: u64) -> Hash {
    let mut bytes = Vec::from(DOMAIN);
    bytes.extend_from_slice(&unix_ms.to_le_bytes());
    hash(&bytes)
}

pub fn sign_dated(secret: &[u8; 32], unix_ms: u64) -> Option<Dated> {
    let signature = olc_crypto::sign(secret, &digest(unix_ms))?.to_vec();
    Some(Dated { unix_ms, signature })
}

/// What a board does with a time that reached it.
#[derive(Debug, PartialEq, Eq)]
pub enum Handling {
    /// Take this time as the clock's, and send `pass`, if any, to every peer by radio and wire.
    Take {
        unix_ms: u64,
        pass: Option<Box<Packet>>,
    },
    Ignore,
}

/// Decides what a board does with `packet`. `passed` is the latest time it passed on, which the
/// caller keeps in memory and sets from `Take`.
pub fn handle(packet: &Packet, gate: &Gate, passed: u64) -> Handling {
    let Body::Dated(dated) = &packet.body else {
        return Handling::Ignore;
    };
    if !gate.signed_by_key(&digest(dated.unix_ms), &dated.signature) {
        return Handling::Ignore;
    }
    // Sent to this board by name, straight from the computer: the board on the cable. Its copy
    // goes one hop and no further, and only when it is news, so a copy sent again by a stranger
    // makes no radio traffic.
    let from_the_cable = packet.to.is_some() && packet.hops == 0;
    let pass = (from_the_cable && dated.unix_ms > passed).then(|| {
        Box::new(Packet {
            to: None,
            hops: MAX_HOPS,
            ..packet.clone()
        })
    });
    Handling::Take {
        unix_ms: dated.unix_ms,
        pass,
    }
}

/// When a board writes the date it holds to flash: once when it first learns the date after a
/// start, then every [`SAVE_EVERY_MS`] while it has one.
#[derive(Default)]
pub struct Keeping {
    saved_at: Option<u64>,
    dated: bool,
}

impl Keeping {
    /// The board learned the date, from the computer or from its own flash at the start. `saved`
    /// says the time came from flash, which holds it already.
    pub fn learned(&mut self, now: u64, saved: bool) {
        if saved && self.saved_at.is_none() {
            self.saved_at = Some(now);
        }
        self.dated = true;
    }

    /// True when the clock's time should be written now. The caller writes it then.
    pub fn due(&mut self, now: u64) -> bool {
        let due = self.dated && self.saved_at.is_none_or(|at| now.saturating_sub(at) >= SAVE_EVERY_MS);
        if due {
            self.saved_at = Some(now);
        }
        due
    }
}

/// A saved time as 16 bytes: the time, then its complement, so a write cut short reads as none.
pub fn record(unix_ms: u64) -> [u8; 16] {
    let mut bytes = [0u8; 16];
    bytes[..8].copy_from_slice(&unix_ms.to_le_bytes());
    bytes[8..].copy_from_slice(&(!unix_ms).to_le_bytes());
    bytes
}

pub fn read_record(bytes: &[u8; 16]) -> Option<u64> {
    let mut time = [0u8; 8];
    let mut check = [0u8; 8];
    time.copy_from_slice(&bytes[..8]);
    check.copy_from_slice(&bytes[8..]);
    let unix_ms = u64::from_le_bytes(time);
    (unix_ms == !u64::from_le_bytes(check)).then_some(unix_ms)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::router::{Forward, Router};

    const SECRET: [u8; 32] = [7; 32];
    const COMPUTER: [u8; 6] = [1; 6];
    const CABLE: [u8; 6] = [2; 6];
    const NOW: u64 = 1_793_800_000_000;

    fn gate(secret: &[u8; 32]) -> Gate {
        Gate::new(CABLE, olc_crypto::public_key(secret), 5)
    }

    fn sent(dated: Dated) -> Packet {
        Packet {
            from: COMPUTER,
            to: Some(CABLE),
            id: 1,
            hops: 0,
            body: Body::Dated(dated),
        }
    }

    #[test]
    fn the_board_on_the_cable_takes_the_time_and_passes_it_one_hop_to_every_board() {
        let packet = sent(sign_dated(&SECRET, NOW).unwrap());
        let Handling::Take { unix_ms, pass: Some(passed) } = handle(&packet, &gate(&SECRET), 0) else {
            panic!("the board on the cable kept the time to itself");
        };
        assert_eq!(unix_ms, NOW);
        // A board heard only by radio takes it, and nothing goes further.
        let mut router = Router::new([3; 6], 0);
        let verdict = router.incoming(77, &passed);
        assert!(verdict.for_me);
        assert_eq!(verdict.forward, Forward::Nowhere);
        assert_eq!(handle(&passed, &gate(&SECRET), 0), Handling::Take { unix_ms: NOW, pass: None });
        assert_eq!(Packet::decode(&passed.encode()).as_ref(), Some(&*passed));
    }

    #[test]
    fn a_time_another_key_signed_or_anyone_changed_is_not_taken() {
        let stranger = sent(sign_dated(&[8; 32], NOW).unwrap());
        assert_eq!(handle(&stranger, &gate(&SECRET), 0), Handling::Ignore);
        let mut later = sign_dated(&SECRET, NOW).unwrap();
        later.unix_ms += 3_600_000;
        assert_eq!(handle(&sent(later), &gate(&SECRET), 0), Handling::Ignore);
        let untrusting = Gate::new(CABLE, None, 0);
        assert_eq!(handle(&sent(sign_dated(&SECRET, NOW).unwrap()), &untrusting, 0), Handling::Ignore);
    }

    #[test]
    fn a_copy_sent_again_crosses_the_radio_no_more() {
        let packet = sent(sign_dated(&SECRET, NOW).unwrap());
        assert_eq!(handle(&packet, &gate(&SECRET), NOW), Handling::Take { unix_ms: NOW, pass: None });
    }

    #[test]
    fn a_board_saves_on_learning_the_date_then_every_minute() {
        let mut keeping = Keeping::default();
        assert!(!keeping.due(1_000), "no date, nothing to save");
        keeping.learned(2_000, false);
        assert!(keeping.due(2_000));
        assert!(!keeping.due(2_000 + SAVE_EVERY_MS - 1));
        assert!(keeping.due(2_000 + SAVE_EVERY_MS));
        let mut restarted = Keeping::default();
        restarted.learned(500, true);
        assert!(!restarted.due(600), "what it read from flash is there already");
        assert!(restarted.due(500 + SAVE_EVERY_MS));
    }

    #[test]
    fn a_record_cut_short_reads_as_none() {
        assert_eq!(read_record(&record(NOW)), Some(NOW));
        let mut cut = record(NOW);
        cut[12..].fill(0xFF);
        assert_eq!(read_record(&cut), None);
        assert_eq!(read_record(&[0xFF; 16]), None);
    }
}
