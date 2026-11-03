//! The channels this node has heard of (BOLT 7) and routes through them. An announcement counts
//! once its funding output stands unspent on the chain with the keys it names.
use crate::script;
use crate::wire::{Announcement, ChannelTerms, Hop, NodeId};
use olc_core::{OutPoint, TxOut};
use olc_crypto::Hash;
use olc_lang::GAMMA;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// Channels remembered. Gossip comes from anyone in reach.
const MOST_EDGES: usize = 4_096;
/// Hops a route may take.
const MOST_HOPS: usize = 8;

#[derive(Clone, Debug)]
pub struct Edge {
    pub announcement: Announcement,
    /// The terms each end asks, in the order of the announcement's nodes.
    pub terms: [Option<ChannelTerms>; 2],
}

#[derive(Default)]
pub struct Graph {
    edges: BTreeMap<OutPoint, Edge>,
}

/// A channel this node holds, for the first step of a route.
pub struct Own {
    pub funding: OutPoint,
    pub peer: NodeId,
    pub balance: u64,
}

/// A route found: the first channel, what to offer on it, and the rest for the hops.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Route {
    pub first: OutPoint,
    pub amount: u64,
    pub expiry: u32,
    pub onward: Vec<Hop>,
}

impl Graph {
    pub fn edges(&self) -> impl Iterator<Item = &Edge> {
        self.edges.values()
    }

    pub fn len(&self) -> usize {
        self.edges.len()
    }

    pub fn is_empty(&self) -> bool {
        self.edges.is_empty()
    }

    /// Takes an announcement whose output `coin` finds. True when it is new.
    pub fn take(&mut self, announcement: Announcement, coin: impl Fn(&OutPoint) -> Option<TxOut>) -> bool {
        if self.edges.contains_key(&announcement.funding) || self.edges.len() >= MOST_EDGES || !announcement.signed() {
            return false;
        }
        let lock = script::funding(&announcement.funding_keys[0].0, &announcement.funding_keys[1].0);
        let stands = coin(&announcement.funding)
            .is_some_and(|out| out.language == GAMMA && out.lock == lock && out.value == announcement.capacity);
        if !stands {
            return false;
        }
        self.edges.insert(announcement.funding, Edge { announcement, terms: [None, None] });
        true
    }

    /// Takes newer terms from one end of a known channel. True when they are new.
    pub fn take_terms(&mut self, terms: ChannelTerms) -> bool {
        let Some(edge) = self.edges.get_mut(&terms.funding) else {
            return false;
        };
        let Some(end) = edge.announcement.nodes.iter().position(|node| *node == terms.from) else {
            return false;
        };
        let newer = edge.terms[end].as_ref().is_none_or(|known| known.stamp < terms.stamp);
        if !newer || !terms.signed() {
            return false;
        }
        edge.terms[end] = Some(terms);
        true
    }

    /// Forgets channels whose funding output is spent.
    pub fn prune(&mut self, unspent: impl Fn(&OutPoint) -> bool) {
        self.edges.retain(|point, _| unspent(point));
    }

    /// A route from `source` to `target` for `amount`, fewest hops first. `avoid` are channels that
    /// failed before. `final_expiry` is the expiry the payee gets.
    pub fn route(&self, source: &NodeId, target: &NodeId, amount: u64, final_expiry: u32, own: &[Own], avoid: &BTreeSet<OutPoint>) -> Option<Route> {
        // Breadth first, from the payee back to us, so each hop's fee is known when it is reached.
        let mut came: BTreeMap<NodeId, (OutPoint, NodeId, u64, u32)> = BTreeMap::new();
        let mut seen: BTreeSet<NodeId> = BTreeSet::from([*target]);
        let mut queue = VecDeque::from([(*target, amount, final_expiry, 0usize)]);
        while let Some((node, needed, expiry, hops)) = queue.pop_front() {
            for mine in own.iter().filter(|mine| mine.peer == node && !avoid.contains(&mine.funding)) {
                if mine.balance >= needed {
                    came.insert(*source, (mine.funding, node, needed, expiry));
                    return Some(unwind(source, target, &came));
                }
            }
            if hops >= MOST_HOPS {
                continue;
            }
            for edge in self.edges.values().filter(|edge| !avoid.contains(&edge.announcement.funding)) {
                let nodes = &edge.announcement.nodes;
                let Some(end) = nodes.iter().position(|one| *one == node) else {
                    continue;
                };
                let from = nodes[1 - end];
                let Some(terms) = edge.terms[1 - end].as_ref().filter(|terms| terms.open) else {
                    continue;
                };
                if from == *source || seen.contains(&from) || edge.announcement.capacity < needed {
                    continue;
                }
                seen.insert(from);
                let forwarded = needed.saturating_add(terms.fee_for(needed));
                let earlier = expiry.saturating_add(u32::from(terms.delta));
                came.insert(from, (edge.announcement.funding, node, needed, expiry));
                queue.push_back((from, forwarded, earlier, hops + 1));
            }
        }
        None
    }
}

/// Walks the found steps from the source to the payee.
fn unwind(source: &NodeId, target: &NodeId, came: &BTreeMap<NodeId, (OutPoint, NodeId, u64, u32)>) -> Route {
    let (first, mut next, amount, expiry) = came[source];
    let mut onward = Vec::new();
    while next != *target {
        let Some((channel, after, amount, expiry)) = came.get(&next).copied() else {
            break;
        };
        onward.push(Hop { channel, amount, expiry });
        next = after;
        if onward.len() > MOST_HOPS {
            break;
        }
    }
    Route { first, amount, expiry, onward }
}

/// The digest gossip is known by, so each piece is passed on once.
pub fn gossip_id(bytes: &[u8]) -> Hash {
    olc_crypto::hash(bytes)
}
