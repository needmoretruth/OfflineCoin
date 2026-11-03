//! Gossip (BOLT 7): both ends of an open channel sign its announcement, either sends it with its
//! own terms to everyone in reach, and each node passes on what was new to it.
use crate::bytes::Bytes;
use crate::channel::{Channel, Phase};
use crate::graph::gossip_id;
use crate::node::{ChannelNode, Ground};
use crate::wire::{Announcement, ChannelId, ChannelTerms, NodeId, Wire};
use olc_core::OutPoint;
use olc_crypto::{sign, verify};

/// What this node asks to pass a payment on: a flat part and parts per million.
pub const BASE_FEE: u64 = 1;
pub const RATE: u32 = 100;
/// Blocks kept between the expiry offered to this node and the one it offers on.
pub const DELTA: u16 = 40;
/// Gossip remembered as passed on, so it is not passed again.
const MOST_GOSSIPED: usize = 16_384;

fn ends(channel: &Channel, own: NodeId) -> Option<([NodeId; 2], [Bytes<33>; 2])> {
    let theirs = channel.theirs.as_ref()?.keys.funding;
    let ours = channel.ours.keys.funding;
    match own < channel.peer {
        true => Some(([own, channel.peer], [ours, theirs])),
        false => Some(([channel.peer, own], [theirs, ours])),
    }
}

impl ChannelNode {
    fn announcement_digest(&self, channel: &Channel) -> Option<olc_crypto::Hash> {
        let (nodes, keys) = ends(channel, self.id)?;
        Some(Announcement::digest(&channel.funding(), channel.terms.capacity, &nodes, &keys))
    }

    pub(crate) fn announcement_signed_by_them(&self, channel: &Channel, signature: &Bytes<64>) -> bool {
        self.announcement_digest(channel).is_some_and(|digest| verify(&channel.peer.0, &digest, &signature.0))
    }

    /// Our signature on a channel's announcement, for the other end.
    pub(crate) fn sign_announcement(&mut self, id: ChannelId, _funding: OutPoint) {
        let Some(channel) = self.channels.get(&id) else {
            return;
        };
        let Some(digest) = self.announcement_digest(channel) else {
            return;
        };
        let Some(signature) = sign(&self.secret, &digest) else {
            return;
        };
        let peer = channel.peer;
        self.send(peer, &Wire::AnnounceSigned { id, signature: Bytes(signature) });
    }

    fn announcement_of(&self, channel: &Channel) -> Option<Announcement> {
        let theirs = channel.their_announce?;
        let digest = self.announcement_digest(channel)?;
        let ours = Bytes(sign(&self.secret, &digest)?);
        let (nodes, funding_keys) = ends(channel, self.id)?;
        let signatures = match self.id < channel.peer {
            true => [ours, theirs],
            false => [theirs, ours],
        };
        let announcement = Announcement { funding: channel.funding(), capacity: channel.terms.capacity, nodes, funding_keys, signatures };
        announcement.signed().then_some(announcement)
    }

    fn terms_of(&self, channel: &Channel, now_s: u64) -> ChannelTerms {
        let mut terms = ChannelTerms {
            funding: channel.funding(),
            from: self.id,
            base_fee: BASE_FEE,
            rate: RATE,
            delta: DELTA,
            stamp: now_s,
            open: channel.phase == Phase::Open,
            signature: Bytes([0; 64]),
        };
        terms.sign(&self.secret);
        terms
    }

    /// Once both signatures are in, the channel goes into our own map and out to everyone.
    pub(crate) fn announce_ready(&mut self, id: ChannelId) {
        let Some(channel) = self.channels.get(&id).filter(|channel| channel.phase == Phase::Open) else {
            return;
        };
        if self.graph.edges().any(|edge| edge.announcement.funding == channel.funding()) {
            return;
        }
        let Some(announcement) = self.announcement_of(channel) else {
            return;
        };
        let terms = self.terms_of(channel, self.now_ms / 1_000);
        // Our own channel stands on our own chain: it was seen at its depth before it opened.
        let capacity = announcement.capacity;
        let lock = channel.funding_lock().unwrap_or_default();
        let out = olc_core::TxOut { value: capacity, language: olc_lang::GAMMA, lock };
        self.graph.take(announcement.clone(), |_| Some(out.clone()));
        self.graph.take_terms(terms.clone());
        self.send_all(&Wire::Announce(announcement), None);
        self.send_all(&Wire::Terms(terms), None);
    }

    /// Gossip heard: kept and passed on when it is new.
    pub(crate) fn on_gossip(&mut self, from: NodeId, wire: Wire, ground: &mut dyn Ground) {
        let id = gossip_id(&postcard::to_allocvec(&wire).unwrap_or_default());
        if self.gossiped.contains(&id) {
            return;
        }
        if self.gossiped.len() >= MOST_GOSSIPED {
            self.gossiped.clear();
        }
        self.gossiped.insert(id);
        let new = match &wire {
            Wire::Announce(announcement) => {
                self.graph.take(announcement.clone(), |point| ground.coin(point).map(|(out, _)| out))
            }
            Wire::Terms(terms) => self.graph.take_terms(terms.clone()),
            _ => false,
        };
        if new {
            self.send_all(&wire, Some(from));
        }
    }

    /// Everything we know, for a node that just came in reach.
    pub(crate) fn share_graph(&mut self, to: NodeId) {
        let mut pieces = Vec::new();
        for edge in self.graph.edges() {
            pieces.push(Wire::Announce(edge.announcement.clone()));
            pieces.extend(edge.terms.iter().flatten().cloned().map(Wire::Terms));
        }
        for wire in pieces {
            self.send(to, &wire);
        }
    }

    /// Now and then: our channels' terms again, signatures the other end may have missed, and
    /// channels whose funding output is gone forgotten.
    pub(crate) fn gossip_own(&mut self, ground: &mut dyn Ground) {
        self.graph.prune(|point| ground.coin(point).is_some());
        let now_s = ground.now_s();
        let open: Vec<ChannelId> =
            self.channels.values().filter(|channel| channel.phase == Phase::Open).map(|channel| channel.id).collect();
        for id in open {
            let Some(channel) = self.channels.get(&id) else {
                continue;
            };
            if channel.their_announce.is_none() {
                let funding = channel.funding();
                self.sign_announcement(id, funding);
                continue;
            }
            self.announce_ready(id);
            let Some(channel) = self.channels.get(&id) else {
                continue;
            };
            if let Some(announcement) = self.announcement_of(channel) {
                let terms = self.terms_of(channel, now_s);
                self.graph.take_terms(terms.clone());
                self.send_all(&Wire::Announce(announcement), None);
                self.send_all(&Wire::Terms(terms), None);
            }
        }
    }
}
