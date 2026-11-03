//! The channels this node has heard of (BOLT 7) and routes through them. An announcement counts
//! once its funding output stands unspent on the chain with the keys it names.
use crate::script;
use crate::wire::{Announcement, ChannelTerms, Hop, NodeId, NodeName};
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
    /// The names nodes with a channel here go by.
    names: BTreeMap<NodeId, NodeName>,
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

    /// Takes a newer name of a node that has a channel here; BOLT 7 leaves the rest unheard, so
    /// names stay as few as the channels. True when it is new.
    pub fn take_name(&mut self, name: NodeName) -> bool {
        let known = self.has_node(&name.node);
        let newer = self.names.get(&name.node).is_none_or(|old| old.stamp < name.stamp);
        if !known || !newer || !name.signed() {
            return false;
        }
        self.names.insert(name.node, name);
        true
    }

    /// The name `node` goes by, when it said one.
    pub fn name_of(&self, node: &NodeId) -> Option<&str> {
        self.names.get(node).map(|name| name.name.as_str())
    }

    pub fn names(&self) -> impl Iterator<Item = &NodeName> {
        self.names.values()
    }

    pub fn has_node(&self, node: &NodeId) -> bool {
        self.edges.values().any(|edge| edge.announcement.nodes.contains(node))
    }

    /// Forgets channels whose funding output is spent, and the names of nodes left without one.
    pub fn prune(&mut self, unspent: impl Fn(&OutPoint) -> bool) {
        self.edges.retain(|point, _| unspent(point));
        let nodes: BTreeSet<NodeId> = self.edges.values().flat_map(|edge| edge.announcement.nodes).collect();
        self.names.retain(|node, _| nodes.contains(node));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytes::Bytes;
    use olc_crypto::{public_key, sign};

    fn key(seed: u8) -> ([u8; 32], Bytes<33>) {
        let secret = [seed; 32];
        (secret, Bytes(public_key(&secret).unwrap()))
    }

    /// Two nodes that hold neither funding key name a channel that stands on the chain as theirs.
    #[test]
    fn a_channel_counts_only_when_both_funding_keys_signed_it() {
        let (one, two) = (key(1), key(2));
        let (m, n) = (key(7), key(8));
        let (m, n) = if m.1 < n.1 { (m, n) } else { (n, m) };
        let funding = OutPoint { txid: [9; 32], index: 0 };
        let capacity = 1_000_000;
        let (nodes, funding_keys) = ([m.1, n.1], [one.1, two.1]);
        let digest = Announcement::digest(&funding, capacity, &nodes, &funding_keys);
        let signed = |secret: &[u8; 32]| Bytes(sign(secret, &digest).unwrap());
        // They sign where the funding keys should, with the only keys they have.
        let (signatures, funding_signatures) = ([signed(&m.0), signed(&n.0)], [signed(&m.0), signed(&n.0)]);
        let fake = Announcement { funding, capacity, nodes, funding_keys, signatures, funding_signatures };
        let out = TxOut { value: capacity, language: GAMMA, lock: script::funding(&one.1 .0, &two.1 .0) };
        let mut graph = Graph::default();
        assert!(!graph.take(fake.clone(), |_| Some(out.clone())), "nobody who holds the funding keys signed");
        assert!(graph.is_empty());
        // With both funding keys signed as well, the same channel counts.
        let real = Announcement { funding_signatures: [signed(&one.0), signed(&two.0)], ..fake };
        assert!(graph.take(real, |_| Some(out.clone())));
        // And it goes once its funding output is spent.
        graph.prune(|_| false);
        assert!(graph.is_empty());
    }

    /// A node's name counts once it has a channel on the map, signed by its own key, newer over
    /// older, at most 32 bytes, and goes with its last channel.
    #[test]
    fn a_name_counts_only_from_its_own_node_with_a_channel() {
        let (one, two) = (key(1), key(2));
        let (m, n) = (key(7), key(8));
        let (m, n) = if m.1 < n.1 { (m, n) } else { (n, m) };
        let mut graph = Graph::default();
        let named = NodeName::new(&m.0, m.1, "부엌 컴퓨터", 10);
        assert!(!graph.take_name(named.clone()), "a node with no channel here is not heard");

        let funding = OutPoint { txid: [9; 32], index: 0 };
        let capacity = 1_000_000;
        let (nodes, funding_keys) = ([m.1, n.1], [one.1, two.1]);
        let digest = Announcement::digest(&funding, capacity, &nodes, &funding_keys);
        let signed = |secret: &[u8; 32]| Bytes(sign(secret, &digest).unwrap());
        let real = Announcement {
            funding,
            capacity,
            nodes,
            funding_keys,
            signatures: [signed(&m.0), signed(&n.0)],
            funding_signatures: [signed(&one.0), signed(&two.0)],
        };
        let out = TxOut { value: capacity, language: GAMMA, lock: script::funding(&one.1 .0, &two.1 .0) };
        assert!(graph.take(real, |_| Some(out.clone())));

        let forged = NodeName { name: "가짜".into(), ..named.clone() };
        assert!(!graph.take_name(forged), "a name its node did not sign");
        let by_other = NodeName::new(&n.0, m.1, "가짜", 11);
        assert!(!graph.take_name(by_other), "a name signed by another node");
        assert!(graph.take_name(named.clone()));
        assert_eq!(graph.name_of(&m.1), Some("부엌 컴퓨터"));
        assert!(!graph.take_name(NodeName::new(&m.0, m.1, "older", 9)), "an older name");
        let long = NodeName::new(&m.0, m.1, "가나다라마바사아자차카타파하", 12);
        assert_eq!(long.name, "가나다라마바사아자차", "cut to 32 bytes at a letter's end");
        assert!(graph.take_name(long));
        let name = "a".repeat(33);
        let signature = Bytes(sign(&m.0, &NodeName::digest(&m.1, 13, &name)).unwrap());
        assert!(!graph.take_name(NodeName { node: m.1, name, stamp: 13, signature }), "33 bytes, though signed");

        graph.prune(|_| false);
        assert_eq!(graph.name_of(&m.1), None, "the name goes with the last channel");
    }
}
