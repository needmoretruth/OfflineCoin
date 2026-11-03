//! Every channel of one node: what it hears, what it sends, its files and its orders. A step that
//! changes a channel is saved before anything it leads to leaves, so a restart at any moment
//! carries on where the files say, and a message the other side missed is sent again.
use crate::bytes::{hex, Bytes};
use crate::channel::{Channel, Phase};
use crate::graph::Graph;
use crate::opening::Ask;
use crate::store::{fresh_secret, Store, Writer};
use crate::wire::{ChannelId, Envelope, NodeId, Wire, EVERYONE};
use olc_core::{Address, OutPoint, Tx, TxOut};
use olc_crypto::{public_key, Hash};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::Path;

/// Blocks on top of the funding transaction before a channel opens.
pub const DEPTH: u64 = 3;
/// Blocks a side waits for its own coins after closing alone, unless the opener asks otherwise.
pub const DELAY: u16 = 144;
/// Outputs below this go to the fee.
pub const DUST: u64 = 546;
pub(crate) const HELLO_EVERY_MS: u64 = 5_000;
const RESEND_EVERY_MS: u64 = 3_000;
/// An open the other side has not agreed to by then fails, and the coins held for it go back to
/// the wallet. Nothing was on the chain yet, and the other side never had the funding signed.
pub const GIVE_UP_MS: u64 = 10 * 60 * 1_000;
const GOSSIP_EVERY_MS: u64 = 30_000;
const PUBLISH_EVERY_MS: u64 = 10_000;
const MOST_SAID: usize = 64;
const MOST_PEERS: usize = 256;

/// What a channel node needs from the node it lives in: the chain, the wallet and the pool.
pub trait Ground {
    fn height(&self) -> u64;
    /// The id and transactions of the best chain's block at `height`.
    fn block_at(&self, height: u64) -> Option<(Hash, Vec<Tx>)>;
    /// An unspent output and the height of its block.
    fn coin(&self, point: &OutPoint) -> Option<(TxOut, u64)>;
    /// What signatures in the next block name.
    fn coin_id(&self) -> Option<Hash>;
    fn genesis(&self) -> Hash;
    /// The fee the coin asks of a transaction.
    fn fee(&self) -> u64;
    /// Where this node's coins go.
    fn pay_to(&self) -> Address;
    /// The wallet's coins a funding payment of `amount` would spend now, none already held.
    fn pick_funding(&mut self, amount: u64) -> Result<Vec<OutPoint>, String>;
    /// A signed payment of `amount` to `lock` in gamma from exactly the coins `from`, not sent.
    fn fund(&mut self, lock: &[u8], amount: u64, from: &[OutPoint]) -> Result<Tx, String>;
    /// Checks a transaction as every node would and sends it.
    fn publish(&mut self, tx: &Tx) -> Result<(), String>;
    fn waiting(&self, id: &Hash) -> bool;
    /// Seconds since 1970.
    fn now_s(&self) -> u64;
}

/// A message to send on the links.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Out {
    To(NodeId, Vec<u8>),
    All { bytes: Vec<u8>, except: Option<NodeId> },
}

/// Something worth telling the person.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Said {
    Opened(ChannelId),
    Refused(String),
    Paid { hash: Hash, amount: u64 },
    PayFailed { hash: Hash },
    Received { hash: Hash, amount: u64 },
    Closed(ChannelId),
    Penalty { channel: ChannelId, txid: Hash },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PayState {
    Pending,
    Paid,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Payment {
    pub hash: Hash,
    pub amount: u64,
    pub fee: u64,
    pub payee: NodeId,
    pub channel: ChannelId,
    pub htlc: u64,
    pub state: PayState,
    pub made_s: u64,
    pub text: String,
    pub avoid: Vec<OutPoint>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Made {
    pub invoice: crate::invoice::Invoice,
    pub preimage: Hash,
    pub paid: bool,
}

/// What a node keeps besides its channels.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Book {
    pub invoices: Vec<Made>,
    pub payments: Vec<Payment>,
    /// Preimages learned, by hash. A payment passed on is claimed back with these.
    pub preimages: Vec<(Hash, Hash)>,
    /// The last block the watcher read.
    pub scanned: Option<(u64, Hash)>,
}

#[derive(Clone, Debug, Default)]
pub struct Seen {
    pub name: String,
    pub heard_ms: u64,
}

pub struct ChannelNode {
    pub(crate) store: Store,
    pub(crate) secret: [u8; 32],
    pub id: NodeId,
    pub name: String,
    pub(crate) channels: BTreeMap<ChannelId, Channel>,
    pub(crate) book: Book,
    pub(crate) graph: Graph,
    pub(crate) peers: BTreeMap<NodeId, Seen>,
    pub(crate) outbox: VecDeque<Out>,
    pub(crate) said: VecDeque<Said>,
    pub(crate) now_ms: u64,
    pub(crate) sent_ms: BTreeMap<ChannelId, u64>,
    hello_ms: u64,
    pub(crate) gossip_ms: u64,
    pub(crate) published_ms: BTreeMap<Hash, u64>,
    pub(crate) gossiped: BTreeSet<Hash>,
}


impl ChannelNode {
    /// Opens the node whose files are in `folder`, made the first time through `write`.
    pub fn open(folder: &Path, name: &str, write: Writer) -> Result<Self, String> {
        let store = Store::open(folder, write)?;
        let secret = store.key()?;
        let id = Bytes(public_key(&secret).ok_or("a damaged node key")?);
        let channels = store.channels().into_iter().map(|channel| (channel.id, channel)).collect();
        let book = store.book();
        Ok(Self {
            store,
            secret,
            id,
            name: name.to_string(),
            channels,
            book,
            graph: Graph::default(),
            peers: BTreeMap::new(),
            outbox: VecDeque::new(),
            said: VecDeque::new(),
            now_ms: 0,
            sent_ms: BTreeMap::new(),
            hello_ms: 0,
            gossip_ms: 0,
            published_ms: BTreeMap::new(),
            gossiped: BTreeSet::new(),
        })
    }

    pub fn take_out(&mut self) -> Option<Out> {
        self.outbox.pop_front()
    }

    pub fn take_said(&mut self) -> Option<Said> {
        self.said.pop_front()
    }

    pub(crate) fn say(&mut self, said: Said) {
        if self.said.len() >= MOST_SAID {
            self.said.pop_front();
        }
        self.said.push_back(said);
    }

    pub(crate) fn send(&mut self, to: NodeId, wire: &Wire) {
        let bytes = Envelope::seal(&self.secret, self.id, to, wire);
        self.outbox.push_back(Out::To(to, bytes));
    }

    pub(crate) fn send_all(&mut self, wire: &Wire, except: Option<NodeId>) {
        let bytes = Envelope::seal(&self.secret, self.id, EVERYONE, wire);
        self.gossiped.insert(crate::graph::gossip_id(&postcard::to_allocvec(wire).unwrap_or_default()));
        self.outbox.push_back(Out::All { bytes, except });
    }

    /// Saves a channel, then sends what its step returned. A channel that cannot be saved goes
    /// back to how it was and nothing leaves.
    pub(crate) fn keep(&mut self, before: Option<Channel>, mut channel: Channel, wire: Option<Wire>) -> bool {
        // Every change is saved here, so the moment a channel ends is noted here once.
        let ended = matches!(channel.phase, Phase::Closed | Phase::Failed);
        if ended && channel.ended_ms.is_none() && self.now_ms > 0 {
            channel.ended_ms = Some(self.now_ms);
        }
        if let Err(error) = self.store.save_channel(&channel) {
            eprintln!("channel {}: {error}", hex(&channel.id[..4]));
            if let Some(before) = before {
                self.channels.insert(before.id, before);
            }
            return false;
        }
        let (id, peer) = (channel.id, channel.peer);
        self.channels.insert(id, channel);
        if let Some(wire) = wire {
            self.sent_ms.insert(id, self.now_ms);
            self.send(peer, &wire);
        }
        true
    }

    pub(crate) fn save_book(&self) {
        if let Err(error) = self.store.save_book(&self.book) {
            eprintln!("channels: {error}");
        }
    }

    /// Bytes heard on a link. Gives the node that signed them, so the caller knows who is where.
    pub fn receive(&mut self, bytes: &[u8], ground: &mut dyn Ground) -> Option<NodeId> {
        let (from, to, wire) = Envelope::open(bytes)?;
        if from == self.id || (to != self.id && to != EVERYONE) {
            return None;
        }
        if self.peers.len() >= MOST_PEERS && !self.peers.contains_key(&from) {
            let oldest = self.peers.iter().min_by_key(|(_, seen)| seen.heard_ms).map(|(node, _)| *node);
            oldest.map(|node| self.peers.remove(&node));
        }
        let returning = self.peers.get(&from).is_none_or(|seen| self.now_ms.saturating_sub(seen.heard_ms) > 3 * HELLO_EVERY_MS);
        let seen = self.peers.entry(from).or_default();
        seen.heard_ms = self.now_ms;
        if returning {
            self.resend_to(&from);
        }
        self.hear(from, wire, ground);
        Some(from)
    }

    fn hear(&mut self, from: NodeId, wire: Wire, ground: &mut dyn Ground) {
        match wire {
            Wire::Hello { name } => {
                if let Some(seen) = self.peers.get_mut(&from) {
                    seen.name = name.chars().take(32).collect();
                }
                if self.gossip_ms > 0 {
                    self.share_graph(from);
                }
            }
            Wire::Open { id, capacity, delay, fee, dust, coin, side } => {
                self.on_open(from, id, Ask { capacity, delay, fee, dust, coin }, side, ground);
            }
            Wire::Announce(_) | Wire::Terms(_) | Wire::Node(_) => self.on_gossip(from, wire, ground),
            other => self.on_channel_wire(from, other, ground),
        }
    }

    fn on_open(&mut self, from: NodeId, id: ChannelId, ask: Ask, side: crate::wire::Opening, ground: &mut dyn Ground) {
        if let Some(known) = self.channels.get(&id) {
            if known.peer == from && known.phase == Phase::Accepted {
                if let Some(wire) = known.last_sent.clone() {
                    self.send(from, &wire);
                }
            }
            return;
        }
        let ours = ground.coin_id();
        let refused = match ours {
            _ if ask.fee != ground.fee() => Some("the fee differs from this coin's"),
            Some(coin) if coin == ask.coin => None,
            _ => Some("another coin"),
        };
        let answer = match refused {
            Some(why) => Err(why),
            None => match fresh_secret() {
                Ok(seed) => Channel::answer(id, from, ask, side, seed, ground.pay_to(), self.now_ms),
                Err(_) => Err("no random numbers here"),
            },
        };
        match answer {
            Ok((channel, wire)) => {
                self.keep(None, channel, Some(wire));
            }
            Err(why) => self.send(from, &Wire::Refuse { id, why: why.to_string() }),
        }
    }
    fn on_accept(&mut self, mut channel: Channel, side: crate::wire::Opening, ground: &mut dyn Ground) {
        let before = channel.clone();
        let lock = match channel.on_accept(side) {
            Ok(lock) => lock,
            Err(_) => {
                self.channels.insert(before.id, before);
                return;
            }
        };
        let funded = ground.fund(&lock, channel.terms.capacity, &channel.held).map_err(|_| "not enough coins").and_then(|tx| channel.funded(tx));
        match funded {
            Ok(wire) => {
                self.keep(Some(before), channel, Some(wire));
            }
            Err(why) => {
                channel.phase = Phase::Failed;
                let peer = channel.peer;
                self.say(Said::Refused(why.to_string()));
                self.keep(Some(before), channel.clone(), None);
                self.send(peer, &Wire::Refuse { id: channel.id, why: why.to_string() });
            }
        }
    }

    fn on_channel_wire(&mut self, from: NodeId, wire: Wire, ground: &mut dyn Ground) {
        let id = match &wire {
            Wire::Accept { id, .. }
            | Wire::Refuse { id, .. }
            | Wire::Funding { id, .. }
            | Wire::FundingSigned { id, .. }
            | Wire::Offer { id, .. }
            | Wire::Sign { id, .. }
            | Wire::SignRevoke { id, .. }
            | Wire::Revoke { id, .. }
            | Wire::Close { id, .. }
            | Wire::CloseSigned { id, .. }
            | Wire::AnnounceSigned { id, .. } => *id,
            _ => return,
        };
        let Some(mut channel) = self.channels.remove(&id) else {
            return;
        };
        if channel.peer != from {
            self.channels.insert(id, channel);
            return;
        }
        let before = channel.clone();
        let we_win = self.id < from;
        let result: Result<Option<Wire>, &str> = match wire {
            Wire::Accept { side, .. } => {
                self.on_accept(channel, side, ground);
                return;
            }
            Wire::Refuse { why, .. } => {
                if matches!(channel.phase, Phase::Asked | Phase::Accepted | Phase::FundingSent) {
                    channel.phase = Phase::Failed;
                    self.say(Said::Refused(why));
                }
                Ok(None)
            }
            Wire::Funding { funding, signature, .. } => channel.on_funding(funding, signature),
            Wire::FundingSigned { signature, .. } => match channel.on_funding_signed(signature) {
                Ok(Some(tx)) => {
                    if !self.keep(Some(before), channel, None) {
                        return;
                    }
                    let _ = ground.publish(&tx);
                    self.published_ms.insert(tx.id(), self.now_ms);
                    return;
                }
                other => other.map(|_| None),
            },
            Wire::Offer { number, updates, .. } => {
                self.learn_preimages(&channel, &updates);
                channel.on_offer(number, updates, we_win)
            }
            Wire::Sign { number, signature, htlcs, .. } => channel.on_sign(number, signature, htlcs),
            Wire::SignRevoke { number, signature, htlcs, secret, next, .. } => channel.on_sign_revoke(number, signature, htlcs, secret, next),
            Wire::Revoke { number, secret, next, .. } => channel.on_revoke(number, secret, next).map(|_| None),
            Wire::Close { number, signature, .. } => match channel.on_close(number, signature, we_win) {
                Ok(Some((wire, tx))) => {
                    if !self.keep(Some(before), channel, Some(wire)) {
                        return;
                    }
                    let _ = ground.publish(&tx);
                    return;
                }
                other => other.map(|_| None),
            },
            Wire::CloseSigned { signature, .. } => match channel.on_close_signed(signature) {
                Ok(Some(tx)) => {
                    if !self.keep(Some(before), channel, None) {
                        return;
                    }
                    let _ = ground.publish(&tx);
                    return;
                }
                other => other.map(|_| None),
            },
            Wire::AnnounceSigned { signature, funding_signature, .. } => {
                if self.announcement_signed_by_them(&channel, &signature, &funding_signature) {
                    channel.their_announce = Some(signature);
                    channel.their_funding_announce = Some(funding_signature);
                }
                Ok(None)
            }
            _ => Ok(None),
        };
        match result {
            Ok(wire) => {
                let changed = channel != before;
                if changed || wire.is_some() {
                    self.keep(Some(before), channel, wire);
                } else {
                    self.channels.insert(id, channel);
                }
                self.announce_ready(id);
            }
            Err(why) => {
                eprintln!("channel {}: {why}", hex(&id[..4]));
                self.channels.insert(id, before);
            }
        }
    }

    /// Sends again what the other side may have missed while it was away.
    fn resend_to(&mut self, peer: &NodeId) {
        let waiting: Vec<(ChannelId, Wire)> = self
            .channels
            .values()
            .filter(|channel| channel.peer == *peer && !matches!(channel.phase, Phase::Closed | Phase::Failed))
            .filter_map(|channel| channel.last_sent.clone().map(|wire| (channel.id, wire)))
            .collect();
        for (id, wire) in waiting {
            self.sent_ms.insert(id, self.now_ms);
            self.send(*peer, &wire);
        }
    }

    /// Runs now and then: says hello, sends again what got no answer, watches the chain.
    pub fn tick(&mut self, now_ms: u64, ground: &mut dyn Ground) {
        self.now_ms = now_ms;
        if now_ms.saturating_sub(self.hello_ms) >= HELLO_EVERY_MS {
            self.hello_ms = now_ms;
            let hello = Wire::Hello { name: self.name.clone() };
            self.send_all(&hello, None);
        }
        let stuck: Vec<(NodeId, ChannelId, Wire)> = self
            .channels
            .values()
            .filter(|channel| awaiting_answer(channel))
            .filter(|channel| now_ms.saturating_sub(self.sent_ms.get(&channel.id).copied().unwrap_or(0)) >= RESEND_EVERY_MS)
            .filter_map(|channel| channel.last_sent.clone().map(|wire| (channel.peer, channel.id, wire)))
            .collect();
        for (peer, id, wire) in stuck {
            self.sent_ms.insert(id, now_ms);
            self.send(peer, &wire);
        }
        self.give_up(now_ms);
        self.watch(ground);
        // A channel whose funding output is spent leaves the map at once, not at the next gossip.
        self.graph.prune(|point| ground.coin(point).is_some());
        self.confirm(ground);
        self.settle(ground);
        if now_ms.saturating_sub(self.gossip_ms) >= GOSSIP_EVERY_MS {
            self.gossip_ms = now_ms;
            self.gossip_own(ground);
        }
    }

    /// Opens we asked for that the other side left unanswered too long.
    fn give_up(&mut self, now_ms: u64) {
        let silent = |channel: &&Channel| {
            channel.we_opened
                && matches!(channel.phase, Phase::Asked | Phase::FundingSent)
                && now_ms.saturating_sub(channel.made_ms) >= GIVE_UP_MS
        };
        let silent: Vec<ChannelId> = self.channels.values().filter(silent).map(|channel| channel.id).collect();
        for id in silent {
            let Some(before) = self.channels.get(&id).cloned() else {
                continue;
            };
            let mut channel = before.clone();
            channel.phase = Phase::Failed;
            self.say(Said::Refused("no answer".to_string()));
            self.keep(Some(before), channel, None);
        }
    }

    /// Funding transactions reaching their depth, and ones that still need to go out.
    fn confirm(&mut self, ground: &mut dyn Ground) {
        let height = ground.height();
        let confirming: Vec<ChannelId> =
            self.channels.values().filter(|channel| channel.phase == Phase::Confirming).map(|channel| channel.id).collect();
        for id in confirming {
            let Some(channel) = self.channels.get(&id) else {
                continue;
            };
            match ground.coin(&channel.funding()) {
                Some((_, at)) if height + 1 >= at + DEPTH => {
                    let mut channel = channel.clone();
                    channel.phase = Phase::Open;
                    channel.open_height = Some(at);
                    channel.last_sent = None;
                    let funding = channel.funding();
                    self.keep(None, channel, None);
                    self.say(Said::Opened(id));
                    self.sign_announcement(id, funding);
                    self.announce_ready(id);
                }
                Some(_) => {}
                None => {
                    let tx = channel.funding_tx.clone().filter(|_| channel.we_opened);
                    if let Some(tx) = tx {
                        self.publish_now_and_then(&tx, ground);
                    }
                }
            }
        }
    }

    pub(crate) fn publish_now_and_then(&mut self, tx: &Tx, ground: &mut dyn Ground) {
        let id = tx.id();
        if ground.waiting(&id) {
            return;
        }
        let last = self.published_ms.get(&id).copied();
        if last.is_some_and(|last| self.now_ms.saturating_sub(last) < PUBLISH_EVERY_MS) {
            return;
        }
        self.published_ms.insert(id, self.now_ms);
        let _ = ground.publish(tx);
    }
}

/// A step that waits for the other side's answer, which is sent again until it comes.
fn awaiting_answer(channel: &Channel) -> bool {
    match channel.phase {
        Phase::Asked | Phase::FundingSent => true,
        Phase::Open => channel.round.is_some(),
        _ => false,
    }
}
