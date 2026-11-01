//! Carries packets across boards. A board learns which peer leads to whom from what it hears, and
//! floods what it has no route for.
use crate::packet::{BoardId, Packet};
use alloc::collections::{BTreeMap, VecDeque};

const MAX_HOPS: u8 = 4;
const MAX_ROUTES: usize = 32;
const SEEN: usize = 48;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Forward {
    Nowhere,
    Peer(u32),
    All,
    AllExcept(u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Verdict {
    pub for_me: bool,
    pub forward: Forward,
}

pub struct Router {
    me: BoardId,
    routes: BTreeMap<BoardId, u32>,
    seen: VecDeque<(BoardId, u32)>,
    next_id: u32,
}

impl Router {
    pub fn new(me: BoardId, first_id: u32) -> Self {
        Self {
            me,
            routes: BTreeMap::new(),
            seen: VecDeque::new(),
            next_id: first_id,
        }
    }

    pub fn me(&self) -> BoardId {
        self.me
    }

    /// A new id for a packet this node starts.
    pub fn next_id(&mut self) -> u32 {
        self.next_id = self.next_id.wrapping_add(1);
        self.next_id
    }

    /// Where a packet this node starts should go.
    pub fn outgoing(&self, to: Option<BoardId>) -> Forward {
        to.and_then(|board| self.routes.get(&board))
            .map_or(Forward::All, |peer| Forward::Peer(*peer))
    }

    /// Decides what to do with a packet heard from `peer`. Forward it with one more hop.
    pub fn incoming(&mut self, peer: u32, packet: &Packet) -> Verdict {
        let nothing = Verdict {
            for_me: false,
            forward: Forward::Nowhere,
        };
        let key = (packet.from, packet.id);
        if packet.from == self.me || self.seen.contains(&key) {
            return nothing;
        }
        if self.seen.len() >= SEEN {
            self.seen.pop_front();
        }
        self.seen.push_back(key);
        if self.routes.len() < MAX_ROUTES || self.routes.contains_key(&packet.from) {
            self.routes.insert(packet.from, peer);
        }
        if packet.to == Some(self.me) {
            return Verdict {
                for_me: true,
                forward: Forward::Nowhere,
            };
        }
        let forward = match (packet.hops >= MAX_HOPS, packet.to) {
            (true, _) => Forward::Nowhere,
            (false, Some(board)) => match self.routes.get(&board) {
                Some(next) if *next != peer => Forward::Peer(*next),
                _ => Forward::AllExcept(peer),
            },
            (false, None) => Forward::AllExcept(peer),
        };
        Verdict {
            for_me: packet.to.is_none(),
            forward,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packet::{Body, Read};

    fn packet(from: u8, to: Option<u8>, id: u32) -> Packet {
        Packet {
            from: [from; 6],
            to: to.map(|board| [board; 6]),
            id,
            hops: 0,
            body: Body::Read(Read::Status),
        }
    }

    #[test]
    fn a_packet_for_me_stops_here() {
        let mut router = Router::new([1; 6], 0);
        let verdict = router.incoming(5, &packet(2, Some(1), 1));
        assert!(verdict.for_me);
        assert_eq!(verdict.forward, Forward::Nowhere);
    }

    #[test]
    fn a_copy_is_ignored() {
        let mut router = Router::new([1; 6], 0);
        router.incoming(5, &packet(2, None, 1));
        let verdict = router.incoming(6, &packet(2, None, 1));
        assert!(!verdict.for_me);
        assert_eq!(verdict.forward, Forward::Nowhere);
    }

    #[test]
    fn a_packet_for_everyone_is_heard_and_passed_on() {
        let mut router = Router::new([1; 6], 0);
        let verdict = router.incoming(5, &packet(2, None, 1));
        assert!(verdict.for_me);
        assert_eq!(verdict.forward, Forward::AllExcept(5));
    }

    #[test]
    fn the_way_back_is_learned_from_what_was_heard() {
        let mut router = Router::new([1; 6], 0);
        assert_eq!(router.outgoing(Some([2; 6])), Forward::All);
        router.incoming(5, &packet(2, None, 1));
        assert_eq!(router.outgoing(Some([2; 6])), Forward::Peer(5));
        // From peer 9, for the board behind peer 5.
        let verdict = router.incoming(9, &packet(3, Some(2), 2));
        assert_eq!(verdict.forward, Forward::Peer(5));
    }

    #[test]
    fn an_unknown_board_is_looked_for_everywhere_else() {
        let mut router = Router::new([1; 6], 0);
        let verdict = router.incoming(9, &packet(3, Some(4), 2));
        assert_eq!(verdict.forward, Forward::AllExcept(9));
    }

    #[test]
    fn a_packet_stops_after_some_hops() {
        let mut router = Router::new([1; 6], 0);
        let mut far = packet(2, Some(4), 1);
        far.hops = MAX_HOPS;
        assert_eq!(router.incoming(5, &far).forward, Forward::Nowhere);
    }
}
