//! What a person can ask of the channel node, and how it shows its channels, requests and payments.
use crate::bytes::{hex, Bytes, Key};
use crate::channel::{Channel, Phase};
use crate::flow::FINAL_DELTA;
use crate::graph::{Graph, Own, Route};
use crate::invoice::{coin_tag, Invoice, InvoiceError, MOST_NOTE};
use crate::node::{ChannelNode, Ground, Made, PayState, Payment, Seen, DUST, HELLO_EVERY_MS};
use crate::opening::Ask;
use crate::store::fresh_secret;
use crate::wire::{ChannelId, NodeId, Update};
use olc_core::OutPoint;
use olc_crypto::{sha256, Hash};
use std::collections::BTreeSet;

/// How long a request stays good, in seconds.
pub const REQUEST_LASTS: u32 = 3_600;
const MOST_INVOICES: usize = 256;
const MOST_PAYMENTS: usize = 256;

/// Why an order was not carried out. Screens show each in their own words; these are the
/// command line's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    ThisNode,
    NoV2Yet,
    TooLittle,
    NotSaved,
    NoSuchChannel,
    Busy,
    NotOpen,
    NotARequest,
    BadSignature,
    OtherCoin,
    Expired,
    OwnRequest,
    AlreadyPaid,
    NoRoute,
    NoCoins,
}

impl Refusal {
    pub fn english(self) -> &'static str {
        match self {
            Self::ThisNode => "That is this node.",
            Self::NoV2Yet => "This coin cannot take channel payments yet.",
            Self::TooLittle => "That is too little for a channel.",
            Self::NotSaved => "The channel could not be saved.",
            Self::NoSuchChannel => "No such channel.",
            Self::Busy => "A payment is still moving. Try again in a moment.",
            Self::NotOpen => "The channel is not open.",
            Self::NotARequest => "That is not a payment request.",
            Self::BadSignature => "The request's signature does not match.",
            Self::OtherCoin => "The request is for another coin.",
            Self::Expired => "The request has expired.",
            Self::OwnRequest => "That request is your own.",
            Self::AlreadyPaid => "That request is paid or being paid.",
            Self::NoRoute => "No channel path reaches the payee with enough coins.",
            Self::NoCoins => "There are not enough coins in the wallet for that.",
        }
    }
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.english())
    }
}

/// A channel as a screen shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChannelView {
    pub id: ChannelId,
    pub peer: NodeId,
    pub peer_name: String,
    pub phase: Phase,
    pub we_opened: bool,
    pub ours: u64,
    pub theirs: u64,
    pub capacity: u64,
    pub waiting: usize,
    pub funding: OutPoint,
    /// The transaction that spent the funding output, once there is one.
    pub closed_by: Option<Hash>,
    /// Blocks until our own coins can be taken after closing alone.
    pub blocks_left: Option<u64>,
    pub penalty: Option<Hash>,
    pub claims: Vec<Hash>,
    /// Who took the channel to the chain alone: `Some(true)` for us.
    pub forced_by_us: Option<bool>,
    pub cheated: bool,
    /// What has come back to our own address on the chain since the channel closed.
    pub back: u64,
    /// What is on its way back while the close waits for its blocks.
    pub coming: u64,
    /// Blocks our share waits after we close alone.
    pub delay: u16,
}

/// What paying a request would take, said before anyone presses pay.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Quote {
    pub payee: NodeId,
    /// The payee's own name when this node has heard it, else empty.
    pub name: String,
    pub amount: u64,
    /// What the nodes on the way ask on top.
    pub fee: u64,
    pub note: String,
}

/// What came back to our address in the transactions the chain shows for a channel.
fn came_back(channel: &Channel) -> u64 {
    let outputs = channel.chain.seen.iter().flat_map(|(_, tx)| tx.outputs.iter());
    outputs.filter(|out| out.pays(&channel.ours.pay_to)).map(|out| out.value).fold(0, u64::saturating_add)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaymentView {
    pub hash: Hash,
    pub amount: u64,
    pub fee: u64,
    pub state: PayState,
    pub made_s: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvoiceView {
    pub text: String,
    pub amount: u64,
    pub note: String,
    pub paid: bool,
    pub expired: bool,
    pub made_s: u64,
}

/// What we can offer in a channel now: our side less the fee we would pay as opener.
pub fn spendable(channel: &Channel) -> u64 {
    let fee = if channel.we_opened { channel.terms.fee } else { 0 };
    channel.state.ours.saturating_sub(fee)
}

impl ChannelNode {
    /// Opens a channel to `peer` with `amount` of this wallet's coins.
    pub fn open_channel(&mut self, peer: NodeId, amount: u64, delay: u16, ground: &mut dyn Ground) -> Result<ChannelId, Refusal> {
        if peer == self.id {
            return Err(Refusal::ThisNode);
        }
        let coin = ground.coin_id().ok_or(Refusal::NoV2Yet)?;
        let ask = Ask { capacity: amount, delay, fee: ground.fee(), dust: DUST, coin };
        let seed = fresh_secret().map_err(|_| Refusal::NotSaved)?;
        let (mut channel, wire) = Channel::start(peer, ask, seed, ground.pay_to(), self.now_ms).map_err(|_| Refusal::TooLittle)?;
        // The coins are chosen now and written with the channel, so no other payment takes them.
        channel.held = ground.pick_funding(amount).map_err(|_| Refusal::NoCoins)?;
        let id = channel.id;
        match self.keep(None, channel, Some(wire)) {
            true => Ok(id),
            false => Err(Refusal::NotSaved),
        }
    }

    /// Closes together with the other side, or alone with `force`.
    pub fn close(&mut self, id: &ChannelId, force: bool, ground: &mut dyn Ground) -> Result<(), Refusal> {
        let channel = self.channels.get(id).cloned().ok_or(Refusal::NoSuchChannel)?;
        if force {
            return self.force_close(id, ground);
        }
        if channel.phase != Phase::Open {
            return Err(Refusal::NotOpen);
        }
        let mut changed = channel.clone();
        let wire = changed.propose_close().map_err(|_| Refusal::Busy)?;
        match self.keep(Some(channel), changed, Some(wire)) {
            true => Ok(()),
            false => Err(Refusal::NotSaved),
        }
    }

    /// Takes our latest state to the chain alone.
    pub fn force_close(&mut self, id: &ChannelId, ground: &mut dyn Ground) -> Result<(), Refusal> {
        let channel = self.channels.get(id).cloned().ok_or(Refusal::NoSuchChannel)?;
        if !matches!(channel.phase, Phase::Confirming | Phase::Open | Phase::Closing | Phase::ForceClosing) {
            return Err(Refusal::NotOpen);
        }
        let tx = channel.signed_commitment().map_err(|_| Refusal::NotOpen)?;
        let mut changed = channel.clone();
        changed.phase = Phase::ForceClosing;
        changed.round = None;
        changed.queue.clear();
        changed.last_sent = None;
        if !self.keep(Some(channel), changed, None) {
            return Err(Refusal::NotSaved);
        }
        let _ = ground.publish(&tx);
        self.published_ms.insert(tx.id(), self.now_ms);
        Ok(())
    }

    /// A new payment request for `amount`, as text.
    pub fn make_invoice(&mut self, amount: u64, note: &str, ground: &mut dyn Ground) -> Result<String, Refusal> {
        if amount == 0 {
            return Err(Refusal::TooLittle);
        }
        let preimage = fresh_secret().map_err(|_| Refusal::NotSaved)?;
        let invoice = Invoice {
            payee: self.id,
            amount,
            hash: sha256(&preimage),
            made: ground.now_s(),
            lasts: REQUEST_LASTS,
            final_delta: FINAL_DELTA,
            coin: coin_tag(&ground.genesis()),
            note: note.chars().take(MOST_NOTE).collect(),
            signature: Bytes([0; 64]),
        }
        .signed(&self.secret);
        let text = invoice.to_text();
        if self.book.invoices.len() >= MOST_INVOICES {
            self.book.invoices.remove(0);
        }
        self.book.invoices.push(Made { invoice, preimage, paid: false });
        self.store.save_book(&self.book).map_err(|_| Refusal::NotSaved)?;
        Ok(text)
    }

    /// Reads a request without paying it.
    pub fn read_invoice(&self, text: &str, ground: &dyn Ground) -> Result<Invoice, Refusal> {
        Invoice::read(text, &coin_tag(&ground.genesis()), ground.now_s()).map_err(|error| match error {
            InvoiceError::NotOne => Refusal::NotARequest,
            InvoiceError::BadSignature => Refusal::BadSignature,
            InvoiceError::OtherCoin => Refusal::OtherCoin,
            InvoiceError::Expired => Refusal::Expired,
        })
    }

    /// What paying a request would take: who gets it, and the fee of the route found now.
    pub fn quote(&self, text: &str, ground: &dyn Ground) -> Result<Quote, Refusal> {
        let (invoice, route) = self.route_for(text, ground)?;
        let name = self.peers.get(&invoice.payee).map(|seen| seen.name.clone()).unwrap_or_default();
        let fee = route.amount.saturating_sub(invoice.amount);
        Ok(Quote { payee: invoice.payee, name, amount: invoice.amount, fee, note: invoice.note })
    }

    /// The request read and checked, and the route that pays it now.
    fn route_for(&self, text: &str, ground: &dyn Ground) -> Result<(Invoice, Route), Refusal> {
        let invoice = self.read_invoice(text, ground)?;
        if invoice.payee == self.id {
            return Err(Refusal::OwnRequest);
        }
        let taken = self.book.payments.iter().any(|payment| payment.hash == invoice.hash && payment.state != PayState::Failed);
        if taken || self.preimage_of(&invoice.hash).is_some() {
            return Err(Refusal::AlreadyPaid);
        }
        let height = ground.height();
        let final_expiry = u32::try_from(height + 1 + u64::from(invoice.final_delta)).map_err(|_| Refusal::NoRoute)?;
        let own: Vec<Own> = self
            .channels
            .values()
            .filter(|channel| channel.phase == Phase::Open)
            .map(|channel| Own { funding: channel.funding(), peer: channel.peer, balance: spendable(channel) })
            .collect();
        let route = self.graph.route(&self.id, &invoice.payee, invoice.amount, final_expiry, &own, &BTreeSet::new()).ok_or(Refusal::NoRoute)?;
        Ok((invoice, route))
    }

    /// Pays a request through the channels, directly or by way of others.
    pub fn pay(&mut self, text: &str, ground: &mut dyn Ground) -> Result<Hash, Refusal> {
        let (invoice, route) = self.route_for(text, ground)?;
        let mut channel = self.channels.values().find(|channel| channel.funding() == route.first).cloned().ok_or(Refusal::NoRoute)?;
        let before = channel.clone();
        let htlc = channel.next_id;
        if self.book.payments.len() >= MOST_PAYMENTS {
            if let Some(at) = self.book.payments.iter().position(|payment| payment.state != PayState::Pending) {
                self.book.payments.remove(at);
            }
        }
        self.book.payments.push(Payment {
            hash: invoice.hash,
            amount: invoice.amount,
            fee: route.amount.saturating_sub(invoice.amount),
            payee: invoice.payee,
            channel: channel.id,
            htlc,
            state: PayState::Pending,
            made_s: ground.now_s(),
            text: text.trim().to_string(),
            avoid: Vec::new(),
        });
        // The payment is written down before it can leave, so a restart finds it either way.
        self.store.save_book(&self.book).map_err(|_| Refusal::NotSaved)?;
        channel.next_id += 1;
        channel.queue.push(Update::Add { id: htlc, amount: route.amount, hash: invoice.hash, expiry: route.expiry, onward: route.onward });
        if !self.keep(Some(before), channel, None) {
            return Err(Refusal::NotSaved);
        }
        self.settle(ground);
        Ok(invoice.hash)
    }

    pub fn payments(&self) -> Vec<PaymentView> {
        let view = |payment: &Payment| PaymentView {
            hash: payment.hash,
            amount: payment.amount,
            fee: payment.fee,
            state: payment.state,
            made_s: payment.made_s,
        };
        self.book.payments.iter().rev().map(view).collect()
    }

    pub fn invoices(&self, now_s: u64) -> Vec<InvoiceView> {
        let view = |made: &Made| InvoiceView {
            text: made.invoice.to_text(),
            amount: made.invoice.amount,
            note: made.invoice.note.clone(),
            paid: made.paid,
            expired: made.invoice.expired(now_s),
            made_s: made.invoice.made,
        };
        self.book.invoices.iter().rev().map(view).collect()
    }

    pub fn channel(&self, id: &ChannelId) -> Option<&Channel> {
        self.channels.get(id)
    }

    /// Finds a channel by the start of its id in hex.
    pub fn find(&self, start: &str) -> Option<ChannelId> {
        let start = start.trim().to_lowercase();
        let mut found = self.channels.keys().filter(|id| hex(&id[..]).starts_with(&start));
        let first = found.next().copied();
        first.filter(|_| found.next().is_none() && !start.is_empty())
    }

    /// Nodes heard lately that can hold channels, with their names.
    pub fn peers(&self) -> Vec<(NodeId, String)> {
        let recent = |seen: &&Seen| self.now_ms.saturating_sub(seen.heard_ms) <= 3 * HELLO_EVERY_MS;
        let near = self.peers.iter().filter(|(_, seen)| recent(seen));
        near.map(|(node, seen)| (*node, seen.name.clone())).collect()
    }

    /// Finds a node heard lately by the start of its key or by its name.
    pub fn find_peer(&self, text: &str) -> Option<NodeId> {
        let text = text.trim().to_lowercase();
        let peers = self.peers();
        let by_name = peers.iter().find(|(_, name)| !name.is_empty() && name.to_lowercase() == text);
        if let Some((node, _)) = by_name {
            return Some(*node);
        }
        let mut found = peers.iter().filter(|(node, _)| hex(&node.0).starts_with(&text));
        let first = found.next().map(|(node, _)| *node);
        first.filter(|_| found.next().is_none() && text.len() >= 4)
    }

    /// Wallet coins held for channels being opened, from the ask until each opens or fails.
    pub fn held_coins(&self) -> Vec<OutPoint> {
        let opening = |channel: &&Channel| {
            channel.we_opened && matches!(channel.phase, Phase::Asked | Phase::Accepted | Phase::FundingSent | Phase::Confirming)
        };
        self.channels.values().filter(opening).flat_map(|channel| channel.held.iter().copied()).collect()
    }

    /// The channels this node knows of, its own and those it heard of.
    pub fn graph(&self) -> &Graph {
        &self.graph
    }

    pub fn views(&self, height: u64) -> Vec<ChannelView> {
        self.channels.values().map(|channel| self.view_of(channel, height)).collect()
    }

    fn view_of(&self, channel: &Channel, height: u64) -> ChannelView {
        let peer_name = self.peers.get(&channel.peer).map(|seen| seen.name.clone()).unwrap_or_default();
        let chain = &channel.chain;
        // After the other side sent an old balance, every coin in the channel is ours, as every
        // screen that shows this channel says next to the shares.
        let waiting: u64 = channel.state.htlcs.iter().map(|htlc| htlc.amount).sum();
        let (ours, theirs) = match chain.cheated() {
            true => (channel.state.ours.saturating_add(channel.state.theirs).saturating_add(waiting), 0),
            false => (channel.state.ours, channel.state.theirs),
        };
        let back = came_back(channel);
        let coming = match channel.phase {
            Phase::Closing | Phase::ForceClosing => ours.saturating_sub(back),
            _ => 0,
        };
        ChannelView {
            id: channel.id,
            peer: channel.peer,
            peer_name,
            phase: channel.phase,
            we_opened: channel.we_opened,
            ours,
            theirs,
            capacity: channel.terms.capacity,
            waiting: channel.state.htlcs.len(),
            funding: channel.funding(),
            closed_by: chain.seen.first().map(|(_, tx)| tx.id()),
            blocks_left: crate::watch::blocks_left(channel, height),
            penalty: chain.penalty,
            claims: chain.claims.clone(),
            forced_by_us: chain.forced_by_us(),
            cheated: chain.cheated(),
            back,
            coming,
            delay: channel.terms.delay,
        }
    }

    pub fn node_key(&self) -> Key {
        self.id
    }
}
