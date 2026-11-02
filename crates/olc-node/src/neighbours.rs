//! Who is around, whichever way they are heard. A neighbour is one node, however many links reach
//! it, and it has one active link at a time: the best one heard lately. The idea of hearing
//! everyone on every link and refusing to flap between close choices is Babel's (RFC 8966).
use crate::engine::PeerId;
use crate::medium::{Medium, Rank};
use alloc::vec::Vec;

/// Every link says who this node is this often.
pub const HELLO_MS: u64 = 1_000;
/// A link is dead after this long without a word: three Hellos and a little.
pub const SILENT_MS: u64 = 3_500;
/// A better link must be heard this long without a gap before traffic moves back to it.
pub const STEADY_MS: u64 = 30_000;
/// A neighbour silent everywhere keeps its link this long, and a connection stays open: signing
/// a large payment stops a board's every link for seconds, and it comes back on the same one.
/// Traffic moves at [`SILENT_MS`] when another link is alive, so waiting costs nothing.
pub const CLOSE_MS: u64 = 15_000;
/// A link to move to must have been heard this lately. A board that stalls goes quiet on every
/// link, and the one it spoke on last would otherwise look alive for a moment longer.
const FRESH_MS: u64 = 1_500;
const MOST: usize = 32;
/// The top byte of a neighbour's peer id. Link positions never reach it.
const NEIGHBOUR: u32 = 0xFF << 24;

/// One way to reach a neighbour: a link, and its number on that link.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Path {
    pub link: usize,
    pub number: u32,
    heard_ms: u64,
    /// When it was last heard after a silence.
    steady_from_ms: u64,
    /// Whether it was heard lately, as of the last [`Neighbours::settle`].
    pub live: bool,
    /// The connection under it ended, which needs no waiting for silence.
    closed: bool,
}

impl Path {
    fn is_live(&self, now: u64) -> bool {
        self.live && now.saturating_sub(self.heard_ms) < SILENT_MS
    }

    fn is_worth_keeping(&self, now: u64) -> bool {
        !self.closed && now.saturating_sub(self.heard_ms) < CLOSE_MS
    }
}

#[derive(Clone, Debug)]
pub struct Neighbour {
    /// What it calls itself. An older build never says, and stays one neighbour per link.
    pub node: Option<u64>,
    pub peer: PeerId,
    pub paths: Vec<Path>,
    /// The link its traffic goes to.
    pub active: Option<usize>,
    /// How often traffic moved from one link to another.
    pub moves: u32,
}

impl Neighbour {
    fn path(&self, link: usize) -> Option<&Path> {
        let usable = |path: &&Path| path.live || (self.active == Some(link) && !path.closed);
        self.paths.iter().filter(|path| path.link == link).find(usable)
    }

    fn active_path(&self) -> Option<&Path> {
        self.active.and_then(|link| self.path(link))
    }
}

/// Traffic that left one link for another.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Move {
    pub peer: PeerId,
    pub from: Option<usize>,
    pub to: Option<usize>,
}

/// Who on one link should get something sent to the neighbours.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fan {
    Nobody,
    Everyone,
    EveryoneBut(u32),
    Each(Vec<u32>),
}

#[derive(Default)]
pub struct Neighbours {
    list: Vec<Neighbour>,
    /// What each link is, by position.
    media: Vec<Medium>,
    pub rank: Rank,
    next_peer: u32,
}

impl Neighbours {
    pub fn add_link(&mut self, medium: Medium) {
        self.media.push(medium);
    }

    pub fn medium(&self, link: usize) -> Option<Medium> {
        self.media.get(link).copied()
    }

    fn place(&self, link: usize) -> usize {
        self.medium(link).map_or(usize::MAX, |medium| self.rank.place_of(medium))
    }

    /// True when `link` ranks above `other`.
    pub fn beats(&self, link: usize, other: usize) -> bool {
        self.place(link) < self.place(other)
    }

    pub fn list(&self) -> &[Neighbour] {
        &self.list
    }

    fn holder(&self, link: usize, number: u32) -> Option<usize> {
        let found = |one: &Neighbour| one.paths.iter().any(|path| (path.link, path.number) == (link, number));
        self.list.iter().position(found)
    }

    fn add(&mut self, node: Option<u64>) -> usize {
        if self.list.len() >= MOST {
            self.list.remove(0);
        }
        self.next_peer = (self.next_peer + 1) & !NEIGHBOUR;
        let peer = NEIGHBOUR | self.next_peer;
        self.list.push(Neighbour { node, peer, paths: Vec::new(), active: None, moves: 0 });
        self.list.len() - 1
    }

    /// Something came from `number` on `link`. Returns who that is.
    pub fn hear(&mut self, link: usize, number: u32, now: u64) -> PeerId {
        let at = match self.holder(link, number) {
            Some(at) => at,
            None => self.add(None),
        };
        let one = &mut self.list[at];
        match one.paths.iter_mut().find(|path| (path.link, path.number) == (link, number)) {
            Some(path) => {
                if !path.is_live(now) {
                    path.steady_from_ms = now;
                }
                path.heard_ms = now;
                path.live = true;
                path.closed = false;
            }
            None => one.paths.push(Path { link, number, heard_ms: now, steady_from_ms: now, live: true, closed: false }),
        }
        one.peer
    }

    /// A Hello said who `number` on `link` is. Returns the peer id that stopped meaning anything
    /// when that turned out to be a neighbour known some other way.
    pub fn name(&mut self, link: usize, number: u32, node: u64, now: u64) -> Option<PeerId> {
        self.hear(link, number, now);
        let at = self.holder(link, number)?;
        if self.list[at].node == Some(node) {
            return None;
        }
        let Some(known) = self.list.iter().position(|one| one.node == Some(node)) else {
            self.list[at].node = Some(node);
            return None;
        };
        // A number on a link can pass to another node, as a connection slot does.
        let one = &mut self.list[at];
        let held = one.paths.iter().position(|path| (path.link, path.number) == (link, number))?;
        let path = one.paths.remove(held);
        let empty = one.paths.is_empty();
        self.list[known].paths.push(path);
        if empty {
            return Some(self.list.remove(at).peer);
        }
        None
    }

    /// A connection closed, so there is no need to wait for silence.
    pub fn lost(&mut self, link: usize, number: u32) {
        if let Some(at) = self.holder(link, number) {
            for path in &mut self.list[at].paths {
                if (path.link, path.number) == (link, number) {
                    path.live = false;
                    path.closed = true;
                }
            }
        }
    }

    /// A link was turned off here, so nobody can be reached by it from now on.
    pub fn lost_link(&mut self, medium: Medium) {
        for path in self.list.iter_mut().flat_map(|one| &mut one.paths) {
            if self.media.get(path.link) == Some(&medium) {
                path.live = false;
                path.closed = true;
            }
        }
    }

    /// Picks each neighbour's link, and says where traffic moved.
    pub fn settle(&mut self, now: u64) -> Vec<Move> {
        let mut moves = Vec::new();
        for at in 0..self.list.len() {
            let next = self.choice(&self.list[at], now);
            let one = &mut self.list[at];
            for path in &mut one.paths {
                path.live = path.is_live(now);
            }
            if next != one.active {
                if one.active.is_some() && next.is_some() {
                    one.moves += 1;
                }
                moves.push(Move { peer: one.peer, from: one.active, to: next });
                one.active = next;
            }
        }
        moves
    }

    fn choice(&self, one: &Neighbour, now: u64) -> Option<usize> {
        let live = || one.paths.iter().filter(|path| path.is_live(now));
        let by_rank = |path: &&Path| self.place(path.link);
        match one.active_path().filter(|path| path.is_live(now)) {
            // Moving away is at once, to whatever is alive. With nothing alive it stays put.
            None => {
                let fresh = |path: &&Path| now.saturating_sub(path.heard_ms) < FRESH_MS;
                let kept = one.paths.iter().filter(|path| Some(path.link) == one.active);
                let kept = kept.filter(|path| path.is_worth_keeping(now)).map(|path| path.link).next();
                let best = |only_fresh: bool| live().filter(|path| !only_fresh || fresh(path)).min_by_key(by_rank);
                best(true).map(|path| path.link).or(kept).or(best(false).map(|path| path.link))
            }
            Some(active) => {
                let steady = |path: &&Path| now.saturating_sub(path.steady_from_ms) >= STEADY_MS;
                let better = live()
                    .filter(|path| self.beats(path.link, active.link))
                    .filter(steady)
                    .min_by_key(by_rank);
                Some(better.unwrap_or(active).link)
            }
        }
    }

    /// Where a message for this neighbour goes now.
    pub fn route(&self, peer: PeerId) -> Option<(usize, u32)> {
        let one = self.list.iter().find(|one| one.peer == peer)?;
        one.active_path().map(|path| (path.link, path.number))
    }

    /// The medium of the link a neighbour's traffic uses.
    pub fn medium_of(&self, peer: PeerId) -> Option<Medium> {
        self.route(peer).and_then(|(link, _)| self.medium(link))
    }

    /// The neighbour behind a peer id or a link and number.
    pub fn peer_of(&self, link: usize, number: u32) -> Option<PeerId> {
        self.holder(link, number).map(|at| self.list[at].peer)
    }

    /// True when `peer` is a neighbour's id, not one of a link and number.
    pub fn is_neighbour(peer: PeerId) -> bool {
        peer & NEIGHBOUR == NEIGHBOUR
    }

    /// Whom to send to on one link so that everyone hears once. A link where nobody has been
    /// heard gets everything: that is how the first word reaches a new node.
    pub fn fan(&self, link: usize, except: Option<PeerId>) -> Fan {
        let on_link = || self.list.iter().filter_map(|one| Some((one, one.path(link)?)));
        let wanted = |one: &Neighbour| one.active == Some(link) && Some(one.peer) != except;
        if on_link().next().is_none() {
            return Fan::Everyone;
        }
        let each: Vec<u32> = on_link().filter(|(one, _)| wanted(one)).map(|(_, path)| path.number).collect();
        let mut others = on_link().filter(|(one, _)| !wanted(one)).map(|(_, path)| path.number);
        match (each.is_empty(), others.next(), others.next()) {
            (true, _, _) => Fan::Nobody,
            (false, None, _) => Fan::Everyone,
            (false, Some(other), None) if on_link().any(|(one, _)| Some(one.peer) == except && one.path(link).is_some_and(|path| path.number == other)) => {
                Fan::EveryoneBut(other)
            }
            _ => Fan::Each(each),
        }
    }

    /// Neighbours whose traffic goes through `link` right now.
    pub fn count_on(&self, link: usize) -> usize {
        self.list.iter().filter(|one| one.active == Some(link)).count()
    }

    /// Neighbours with a link that works.
    pub fn count(&self) -> usize {
        self.list.iter().filter(|one| one.active.is_some()).count()
    }

    /// The link a named neighbour's traffic uses, if it is heard there now.
    pub fn active_of(&self, node: u64) -> Option<usize> {
        let one = self.list.iter().find(|one| one.node == Some(node))?;
        one.active.filter(|link| one.paths.iter().any(|path| path.link == *link && path.live))
    }

    /// True when a connection to a named neighbour has been silent long enough to close.
    pub fn is_gone_from(&self, node: u64, link: usize, now: u64) -> bool {
        let paths = self.list.iter().filter(|one| one.node == Some(node)).flat_map(|one| &one.paths);
        paths.filter(|path| path.link == link).all(|path| now.saturating_sub(path.heard_ms) >= CLOSE_MS)
    }

    /// True when a named neighbour is heard on `link`.
    pub fn is_heard_on(&self, node: u64, link: usize) -> bool {
        self.list.iter().any(|one| one.node == Some(node) && one.path(link).is_some())
    }
}

#[cfg(test)]
#[path = "neighbours_tests.rs"]
mod tests;
