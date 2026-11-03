//! Pool work for a board a computer reaches only through another board. The computer signs the
//! work with its control key and sends it to the board on its cable, which passes it one hop on
//! to the board it is for. That board grinds it and sends the headers it finds back the same way.
//!
//! The signature keeps a stranger in radio reach from putting a board to work for someone else.
//! Nothing counts lent work, so a copy sent again only sets a board on a job that already ended
//! until the next one comes, which a jammed radio could do as well.
use crate::gate::Gate;
use crate::packet::{Body, Packet};
use crate::router::MAX_HOPS;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use olc_crypto::{hash, Hash};
use serde::{Deserialize, Serialize};

/// The address a board mines to. It names the board here, since that is what the computer hears.
pub type Address = [u8; 20];

const DOMAIN: &[u8] = b"OfflineCoin lent work 1\n";
/// Lent work that nothing renewed for this long ends, and the board mines alone again. The
/// computer hands out work at least every 20 seconds.
pub const LENT_FOR_MS: u64 = 60_000;
/// Boards a relay remembers at once.
const MOST_HEARD: usize = 16;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lent {
    pub worker: Address,
    /// A header as `BlockHeader::encode` writes it. Empty means stop.
    pub header: Vec<u8>,
    /// What a share must reach.
    pub difficulty: u64,
    pub signature: Vec<u8>,
}

fn digest(worker: &Address, header: &[u8], difficulty: u64) -> Hash {
    let mut bytes = Vec::from(DOMAIN);
    bytes.extend_from_slice(worker);
    bytes.extend_from_slice(&difficulty.to_le_bytes());
    bytes.extend_from_slice(header);
    hash(&bytes)
}

/// Signs work for the board that mines to `worker`. `None` stops it.
pub fn sign_lent(secret: &[u8; 32], worker: Address, work: Option<(Vec<u8>, u64)>) -> Option<Lent> {
    let (header, difficulty) = work.unwrap_or_default();
    let signature = olc_crypto::sign(secret, &digest(&worker, &header, difficulty))?.to_vec();
    Some(Lent {
        worker,
        header,
        difficulty,
        signature,
    })
}

/// The boards a relay heard say what they are, by the address each mines to, with the peer it
/// spoke from.
#[derive(Default)]
pub struct Heard {
    peers: BTreeMap<Address, u32>,
}

impl Heard {
    pub fn heard(&mut self, address: Address, peer: u32) {
        if self.peers.len() >= MOST_HEARD && !self.peers.contains_key(&address) {
            self.peers.pop_first();
        }
        self.peers.insert(address, peer);
    }

    pub fn peer_of(&self, address: &Address) -> Option<u32> {
        self.peers.get(address).copied()
    }
}

/// What a board does with lent work that reached it.
#[derive(Debug, PartialEq, Eq)]
pub enum Handling {
    /// Grind this header to this difficulty for the packet's sender. `None` stops lent work.
    Take(Option<(Vec<u8>, u64)>),
    /// Send this packet to this peer, the board the work is for.
    Pass(u32, Packet),
    Ignore,
}

/// Decides what the board that mines to `me` does with `packet`, which its router gave it.
pub fn handle(packet: &Packet, me: &Address, gate: &Gate, heard: &Heard) -> Handling {
    let Body::Lend(lent) = &packet.body else {
        return Handling::Ignore;
    };
    // Both boards check the signature: the worker so nobody else spends its hashing, the board on
    // the cable so nobody else spends its radio.
    if !gate.signed_by_key(&digest(&lent.worker, &lent.header, lent.difficulty), &lent.signature) {
        return Handling::Ignore;
    }
    if lent.worker == *me {
        let work = (!lent.header.is_empty()).then(|| (lent.header.clone(), lent.difficulty));
        return Handling::Take(work);
    }
    // Only work sent to this board by name goes on, and one hop only: the copy passed on has gone
    // as far as it may, so no other board passes it again or floods it.
    match (packet.to.is_some(), heard.peer_of(&lent.worker)) {
        (true, Some(peer)) => {
            let passed = Packet {
                to: None,
                hops: MAX_HOPS,
                ..packet.clone()
            };
            Handling::Pass(peer, passed)
        }
        _ => Handling::Ignore,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::router::{Forward, Router};
    use alloc::vec;

    const SECRET: [u8; 32] = [7; 32];
    const COMPUTER: [u8; 6] = [1; 6];
    const RELAY: [u8; 6] = [2; 6];
    const ZERO: Address = [9; 20];
    const ZERO_PEER: u32 = 0x00AA_EF90;

    fn gate(secret: &[u8; 32]) -> Gate {
        Gate::new([3; 6], olc_crypto::public_key(secret), 5)
    }

    fn sent(lent: Lent) -> Packet {
        Packet {
            from: COMPUTER,
            to: Some(RELAY),
            id: 1,
            hops: 0,
            body: Body::Lend(lent),
        }
    }

    fn relay() -> Heard {
        let mut heard = Heard::default();
        heard.heard(ZERO, ZERO_PEER);
        heard
    }

    #[test]
    fn the_relay_passes_work_one_hop_on_to_the_board_it_heard() {
        let lent = sign_lent(&SECRET, ZERO, Some((vec![5; 88], 40))).unwrap();
        let Handling::Pass(peer, passed) = handle(&sent(lent), &[4; 20], &gate(&SECRET), &relay()) else {
            panic!("the relay kept the work");
        };
        assert_eq!(peer, ZERO_PEER);
        // The board it is for takes it, and nothing goes further.
        let mut router = Router::new([3; 6], 0);
        let verdict = router.incoming(77, &passed);
        assert!(verdict.for_me);
        assert_eq!(verdict.forward, Forward::Nowhere);
        assert_eq!(handle(&passed, &ZERO, &gate(&SECRET), &Heard::default()), Handling::Take(Some((vec![5; 88], 40))));
        // Its answer finds the way back through the relay it came from.
        assert_eq!(router.outgoing(Some(COMPUTER)), Forward::Peer(77));
        // A copy already passed on is not passed again.
        assert_eq!(handle(&passed, &[4; 20], &gate(&SECRET), &relay()), Handling::Ignore);
    }

    #[test]
    fn work_another_key_signed_or_anyone_changed_is_not_taken() {
        let stranger = sign_lent(&[8; 32], ZERO, Some((vec![5; 88], 40))).unwrap();
        assert_eq!(handle(&sent(stranger.clone()), &ZERO, &gate(&SECRET), &Heard::default()), Handling::Ignore);
        assert_eq!(handle(&sent(stranger), &[4; 20], &gate(&SECRET), &relay()), Handling::Ignore, "the relay passed it on");
        let mut eased = sign_lent(&SECRET, ZERO, Some((vec![5; 88], 40))).unwrap();
        eased.difficulty = 1;
        assert_eq!(handle(&sent(eased), &ZERO, &gate(&SECRET), &Heard::default()), Handling::Ignore);
        let mut other = sign_lent(&SECRET, ZERO, Some((vec![5; 88], 40))).unwrap();
        other.header[0] = 6;
        assert_eq!(handle(&sent(other), &ZERO, &gate(&SECRET), &Heard::default()), Handling::Ignore);
        let untrusting = Gate::new([3; 6], None, 0);
        let lent = sign_lent(&SECRET, ZERO, Some((vec![5; 88], 40))).unwrap();
        assert_eq!(handle(&sent(lent), &ZERO, &untrusting, &Heard::default()), Handling::Ignore);
    }

    #[test]
    fn a_signed_stop_ends_lent_work() {
        let stop = sign_lent(&SECRET, ZERO, None).unwrap();
        assert_eq!(handle(&sent(stop), &ZERO, &gate(&SECRET), &Heard::default()), Handling::Take(None));
    }

    #[test]
    fn work_for_a_board_the_relay_never_heard_goes_nowhere() {
        let lent = sign_lent(&SECRET, [6; 20], Some((vec![5; 88], 40))).unwrap();
        assert_eq!(handle(&sent(lent), &[4; 20], &gate(&SECRET), &relay()), Handling::Ignore);
    }

    #[test]
    fn lent_work_and_shares_survive_the_trip() {
        let lent = sign_lent(&SECRET, ZERO, Some((vec![5; 88], 40))).unwrap();
        let packet = sent(lent);
        assert_eq!(Packet::decode(&packet.encode()), Some(packet));
        let shares = Packet {
            body: Body::Shares {
                worker: ZERO,
                headers: vec![vec![1; 88], vec![2; 88]],
            },
            ..sent(sign_lent(&SECRET, ZERO, None).unwrap())
        };
        assert_eq!(Packet::decode(&shares.encode()), Some(shares));
    }
}
