//! Node logic with no I/O: call it with what happened, then collect what it wants to send.
use crate::event::{Event, Outgoing, Status};
use crate::message::{Heights, Message};
use crate::refused::Refused;
use crate::snapshot::{Fetching, History, Serving};
use crate::spend::Signing;
use crate::sync::Sync;
use alloc::collections::{BTreeMap, BTreeSet, VecDeque};
use alloc::vec::Vec;
use core::cell::Cell;
use olc_core::{
    AddOutcome, Address, Block, Chain, ChainError, CoinConfig, Hash, OutPoint, Plugins, Tx,
    ADDRESS_LANGUAGE,
};
use olc_wallet::Wallet;

pub type PeerId = u32;

const ANNOUNCE_EVERY_MS: u64 = 2_000;
const REFRESH_TEMPLATE_EVERY_MS: u64 = 5_000;
/// Peers remembered. A source address costs nothing to make up, so each new one must not cost
/// memory, and the one heard longest ago gives way.
pub const MOST_PEERS: usize = 64;
/// Answers waiting to leave. Past this a request is dropped, and whoever really wants the answer
/// asks again. Its own news and payments always go out.
pub const MOST_QUEUED: usize = 16;
const MAX_EVENTS: usize = 32;
const PEER_TIMEOUT_MS: u64 = 10_000;
/// A coinbase note of up to this many bytes.
pub const MOST_NOTE: usize = 64;
/// Sectors of the saved state written per tick, so a save never stalls mining.
const STATE_SECTORS_PER_TICK: usize = 1;

pub struct Engine {
    pub(crate) chain: Chain,
    pub(crate) wallet: Wallet,
    pub(crate) pool: Vec<Tx>,
    pub(crate) outbox: VecDeque<Outgoing>,
    events: VecDeque<Event>,
    pub(crate) candidate: Option<Block>,
    candidate_built_ms: u64,
    last_announce_ms: u64,
    /// Added to the device clock. Boards have no real-time clock, so they follow the chain.
    pub(crate) clock_offset_ms: u64,
    pub(crate) now_ms: u64,
    peers: BTreeMap<PeerId, u64>,
    refused: Refused,
    pub(crate) sync: Sync,
    pub(crate) sent: VecDeque<Hash>,
    /// Who mines here gets paid here. `None` pays the key that receives.
    miner: Option<Address>,
    /// Bytes every mined block carries in its coinbase.
    pub(crate) note: Vec<u8>,
    /// Merges a key's small coins into one when set.
    pub(crate) merging: bool,
    /// Signs payments as version 1, the way Satoshi's Bitcoin did, where the coin still takes them.
    pub(crate) version_one: bool,
    pub(crate) signing: Option<Signing>,
    /// Coins a payment channel being opened will spend. No other payment touches them.
    pub(crate) held: Vec<OutPoint>,
    /// Who asked for blocks last, from where and when.
    pub(crate) answered: Option<(PeerId, Option<Hash>, u64)>,
    /// Blocks sent ahead: to whom, the height it asked from, the height of the last one sent.
    pub(crate) ahead: Option<(PeerId, u64, u64)>,
    /// The message being handled came over a link that carries much at once.
    pub(crate) fast_link: bool,
    /// The device clock at the first turn, to give peers a moment before mining on an old tip.
    pub(crate) started_ms: Option<u64>,
    pub(crate) history: History,
    pub(crate) fetching: Option<Fetching>,
    /// The snapshot being handed to a peer.
    pub(crate) serving: Option<Serving>,
    /// A walk down the chain was made this turn, for a peer that named no heights.
    pub(crate) walked: bool,
    /// The wallet's balance and the tip it was counted on. Counting walks every unspent output
    /// and boards ask for the status many times a second, so it runs once per tip.
    balance: Cell<Option<(Hash, u64)>>,
    /// Set when the owner chose to hold headers only.
    pub(crate) light: Option<alloc::boxed::Box<crate::light::Light>>,
    /// Addresses followed without a key.
    pub(crate) watched: Vec<Address>,
}

impl Engine {
    pub fn new(config: CoinConfig, plugins: Plugins, wallet: Wallet) -> Self {
        Self::on_chain(Chain::new(config, plugins), wallet)
    }

    /// A node on a chain that is already open, such as one with a store behind it.
    pub fn on_chain(chain: Chain, wallet: Wallet) -> Self {
        Self {
            chain,
            wallet,
            pool: Vec::new(),
            outbox: VecDeque::new(),
            events: VecDeque::new(),
            candidate: None,
            candidate_built_ms: 0,
            last_announce_ms: 0,
            clock_offset_ms: 0,
            now_ms: 0,
            peers: BTreeMap::new(),
            refused: Refused::default(),
            sync: Sync::default(),
            sent: VecDeque::new(),
            miner: None,
            note: Vec::new(),
            merging: true,
            version_one: false,
            signing: None,
            held: Vec::new(),
            answered: None,
            ahead: None,
            fast_link: false,
            started_ms: None,
            history: History::default(),
            fetching: None,
            serving: None,
            walked: false,
            balance: Cell::new(None),
            light: None,
            watched: Vec::new(),
        }
    }

    pub fn chain(&self) -> &Chain {
        &self.chain
    }

    pub fn chain_mut(&mut self) -> &mut Chain {
        &mut self.chain
    }

    pub fn wallet(&self) -> &Wallet {
        &self.wallet
    }

    /// Changes the wallet. The balance is counted again.
    pub fn wallet_mut(&mut self) -> &mut Wallet {
        self.balance.set(None);
        self.candidate = None;
        &mut self.wallet
    }

    /// Where mined coins go. `None` is the key that receives.
    pub fn set_miner(&mut self, miner: Option<Address>) {
        self.miner = miner;
        self.candidate = None;
    }

    pub fn miner(&self) -> Address {
        self.miner.unwrap_or_else(|| self.wallet.address())
    }

    /// True when mined coins go to an address chosen for this node.
    pub fn miner_is_chosen(&self) -> bool {
        self.miner.is_some()
    }

    /// Sets the bytes each mined block carries. Longer notes are cut to [`MOST_NOTE`].
    pub fn set_note(&mut self, note: &[u8]) {
        self.note = note[..note.len().min(MOST_NOTE)].to_vec();
        self.candidate = None;
    }

    pub fn note(&self) -> &[u8] {
        &self.note
    }

    pub fn set_merging(&mut self, merging: bool) {
        self.merging = merging;
    }

    pub fn merging(&self) -> bool {
        self.merging
    }

    pub fn set_version_one(&mut self, on: bool) {
        self.version_one = on;
    }

    pub fn version_one(&self) -> bool {
        self.version_one
    }

    pub fn peer_ids(&self) -> impl Iterator<Item = PeerId> + '_ {
        self.peers.keys().copied()
    }

    /// A peer turned out to be one already known by another id.
    pub fn forget_peer(&mut self, peer: PeerId) {
        self.peers.remove(&peer);
    }

    pub fn address(&self) -> Address {
        self.wallet.address()
    }

    pub fn status(&self) -> Status {
        self.light_status(Status {
            height: self.chain.height(),
            tip: self.chain.tip(),
            work: self.chain.total_work(),
            difficulty: self.difficulty_now(),
            balance: self.balance(),
            pool: self.pool.len(),
            peers: self.peers.len(),
            syncing_to: self.syncing_to(),
            needs_history: self.history.stuck(),
            assumed_from: self.chain.assumed_from(),
            fetching: self.fetching.as_ref().map(Fetching::progress),
        })
    }

    fn balance(&self) -> u64 {
        let tip = self.chain.tip();
        match self.balance.get() {
            Some((counted_on, balance)) if counted_on == tip => balance,
            _ => {
                let balance = self.wallet.balance(&self.chain);
                self.balance.set(Some((tip, balance)));
                balance
            }
        }
    }

    pub fn take_outgoing(&mut self) -> Option<Outgoing> {
        self.outbox.pop_front()
    }

    pub fn take_event(&mut self) -> Option<Event> {
        self.events.pop_front()
    }

    /// Announces the tip now and then, which is also how peers find each other.
    pub fn tick(&mut self, device_ms: u64) {
        self.now_ms = device_ms;
        self.started_ms.get_or_insert(device_ms);
        self.walked = false;
        if self.light_tick(device_ms) {
            return;
        }
        self.peers
            .retain(|_, seen| device_ms.saturating_sub(*seen) < PEER_TIMEOUT_MS);
        self.follow_chain_clock(device_ms);
        if device_ms.saturating_sub(self.last_announce_ms) >= ANNOUNCE_EVERY_MS {
            self.last_announce_ms = device_ms;
            self.announce(None);
        }
        self.history.tick((self.chain.height(), self.chain.tip()), device_ms);
        self.keep_fetching();
        self.sync.new_turn(device_ms);
        self.sign_a_little();
        self.ask_for_blocks();
        self.drop_quiet_snapshot();
        self.chain.keep_up(STATE_SECTORS_PER_TICK);
    }

    pub fn on_message(&mut self, from: PeerId, message: Message) {
        if self.light_heard(from, &message) {
            return;
        }
        hear_peer(&mut self.peers, from, self.now_ms);
        let asks = matches!(
            message,
            Message::GetBlocks(..)
                | Message::GetSnapshot { .. }
                | Message::GetHeaders(_)
                | Message::GetCoins { .. }
        );
        if asks && self.outbox.len() >= MOST_QUEUED {
            return;
        }
        match message {
            Message::Tip {
                genesis,
                tip,
                previous,
                height,
                work,
            } => {
                if genesis != self.chain.genesis_id() || work <= self.chain.total_work() {
                    return;
                }
                // This is only a claim, so it adds nothing to what is known to be invalid.
                if self.refused.has(&tip) || self.refused.has(&previous) {
                    return;
                }
                let own = (self.chain.height(), self.chain.tip());
                if self.history.lacks(from, own, height, self.now_ms) {
                    return;
                }
                self.sync.heard(from, height, work, self.now_ms);
                self.ask_for_blocks();
            }
            Message::GetBlocks(ids, heights) => self.serve_blocks(from, ids, heights.0),
            Message::Pruned { oldest } => {
                let tip = self.chain.tip();
                let first_time = self.history.pruned(from, oldest, tip, self.now_ms);
                self.sync.drop_peer(from);
                if first_time {
                    self.push_event(Event::NeedsHistory { peer: from });
                }
            }
            Message::GetSnapshot { part } => self.serve_snapshot(from, part),
            Message::SnapshotPart {
                sum,
                part,
                parts,
                bytes,
                ..
            } => self.snapshot_part(from, sum, part, parts, bytes),
            Message::Block(block) => self.receive_block(from, block),
            Message::GetHeaders(named) => self.serve_headers(from, &named),
            Message::GetCoins { address, after } => self.serve_coins(from, address, after),
            Message::Control(_)
            | Message::Who
            | Message::Hello(_)
            | Message::Here(_)
            | Message::Channel(_)
            | Message::Headers(_)
            | Message::Coin(_)
            | Message::CoinsEnd { .. } => {}
            Message::Tx(tx) => {
                self.receive_tx(Some(from), tx);
            }
        }
    }

    /// The height this node is catching up to, while it is well behind it. It does not mine then.
    pub fn syncing_to(&self) -> Option<u64> {
        let (height, work) = (self.chain.height(), self.chain.total_work());
        self.sync.behind(height, work, self.now_ms).filter(|_| self.sync.moving(self.now_ms))
    }

    /// True while this node should not mine: it is catching up, or it has just started on a
    /// tip too old to be the network's and has not heard from it yet. As Bitcoin Core refuses
    /// `getblocktemplate` in its initial download, a block made then is made on a tip the
    /// network left long ago.
    pub fn catching_up(&self) -> bool {
        self.syncing_to().is_some() || self.looking_around(self.now_ms)
    }

    /// Like [`Engine::on_message`], for a message that came over a link that carries much at
    /// once: a cable to a computer, or TCP. A peer catching up there gets blocks sent ahead.
    pub fn on_message_over(&mut self, from: PeerId, message: Message, fast_link: bool) {
        self.fast_link = fast_link;
        self.on_message(from, message);
        self.fast_link = false;
    }

    fn ask_for_blocks(&mut self) {
        let Some(peer) = self.sync.next_ask(self.chain.total_work(), self.now_ms) else {
            return;
        };
        let mut locator = self.chain.locator();
        if let Some(position) = self.sync.position() {
            locator.insert(0, position);
        }

        let (heights, ids) = locator.into_iter().unzip();
        let message = Message::GetBlocks(ids, Heights(heights));
        self.outbox.push_back(Outgoing::To(peer, message));
    }

    /// Runs up to `attempts` sealing attempts and adds the block if one succeeds.
    pub fn mine(&mut self, attempts: u32, device_ms: u64) -> bool {
        if self.light.is_some() || self.syncing_to().is_some() || self.looking_around(device_ms) {
            return false;
        }
        self.refresh_candidate(device_ms);
        let Some(block) = self.candidate.as_mut() else {
            return false;
        };
        if !self
            .chain
            .plugins()
            .consensus
            .try_seal(&mut block.header, attempts)
        {
            return false;
        }
        let Some(block) = self.candidate.take() else {
            return false;
        };
        let height = self.chain.height() + 1;
        let old_tip = self.chain.tip();
        if self.chain.add_block(block).is_err() {
            return false;
        }
        self.push_event(Event::Mined { height });
        self.after_tip_change(old_tip);
        self.announce(None);
        true
    }

    fn receive_block(&mut self, from: PeerId, block: Block) {
        let id = block.id();
        let tip_before = self.chain.tip();
        self.add_block(from, block);
        // Blocks that were waiting for this one can connect now.
        let mut ready = self.chain.take_orphans_of(&id);
        while let Some(child) = ready.pop() {
            let child_id = child.id();
            self.add_block(from, child);
            ready.extend(self.chain.take_orphans_of(&child_id));
        }
        if self.chain.tip() != tip_before {
            self.after_tip_change(tip_before);
            if !self.sync.is_active() {
                self.announce(Some(from));
            }
        }
        self.ask_for_blocks();
    }

    fn add_block(&mut self, from: PeerId, block: Block) {
        let id = block.id();
        if self.refused.has(&id) || self.refused.has(&block.header.previous) {
            self.refused.add(id);
            self.stop_following(from);
            return;
        }
        if self.is_from_the_future(&block) {
            // Bitcoin's time-too-new: not stored and not held against anyone, so it is taken once
            // its time is near. The node still refuses it now and says so, which lets a watcher
            // learn the rule without the block ever joining the chain.
            self.push_event(Event::BlockRejected { reason: ChainError::BadTime });
            return;
        }
        let height = block.txs.first().and_then(|tx| tx.coinbase_height);
        let outcome = self.chain.add_block(block);
        let known = outcome == Err(ChainError::AlreadyKnown);
        let connected = match outcome {
            Ok(AddOutcome::Orphan { .. }) => None,
            Err(_) if !known => None,
            _ => height.map(|height| (height, id)),
        };
        self.sync.arrived(connected, known, self.now_ms);
        match outcome {
            Ok(AddOutcome::NewTip) => {
                let height = self.chain.height();
                self.push_event(Event::BlockAccepted { height });
            }
            Ok(AddOutcome::Reorganized { replaced }) => {
                let height = self.chain.height();
                self.push_event(Event::Reorganized { replaced, height });
            }
            Ok(AddOutcome::SideBranch) | Ok(AddOutcome::Orphan { .. }) => {}
            Err(ChainError::AlreadyKnown | ChainError::StoreFailed) => {}
            Err(ChainError::ReorgTooDeep) => {
                self.stop_following(from);
                self.push_event(Event::ReorgRefused);
                let tip = self.chain.tip();
                self.history.pruned(from, self.chain.height() + 2, tip, self.now_ms);
            }
            // The bytes do not match their own header, which a damaged line does as well as a
            // dishonest peer, and says nothing about the tip being followed.
            Err(reason @ (ChainError::BadSeal | ChainError::BadMerkleRoot)) => {
                self.refused.add_invalid(id, reason);
                self.push_event(Event::BlockRejected { reason });
            }
            Err(reason) => {
                self.refused.add_invalid(id, reason);
                self.stop_following(from);
                self.push_event(Event::BlockRejected { reason });
            }
        }
    }

    /// A peer whose chain holds an invalid block is no chain to catch up to. It stays a peer.
    fn stop_following(&mut self, peer: PeerId) {
        self.sync.give_up_on(peer, self.now_ms);
    }

    /// What the blocks that joined the best chain paid this wallet, leaving out mining rewards and
    /// the change of its own payments.
    fn received_since(&self, old_tip: &Hash) -> u64 {
        let (_, joined) = self.chain.branches_from(old_tip);
        joined
            .iter()
            .flat_map(|block| &block.txs)
            .filter(|tx| !tx.is_coinbase() && !self.sent.contains(&tx.id()))
            .flat_map(|tx| &tx.outputs)
            .filter(|output| output.language == ADDRESS_LANGUAGE && self.wallet.owns(&output.lock))
            .map(|output| output.value)
            .fold(0, u64::saturating_add)
    }

    pub(crate) fn after_tip_change(&mut self, old_tip: Hash) {
        self.candidate = None;
        let amount = self.received_since(&old_tip);
        if amount > 0 {
            self.push_event(Event::PaymentReceived { amount });
        }
        let (left, joined) = self.chain.branches_from(&old_tip);
        if left.is_empty() {
            self.drop_spent();
            self.merge_coins();
            return;
        }
        let confirmed: BTreeSet<Hash> = joined.iter().flat_map(|b| &b.txs).map(Tx::id).collect();
        let returning: Vec<Tx> = left
            .iter()
            .flat_map(|block| &block.txs)
            .filter(|tx| !tx.is_coinbase() && !confirmed.contains(&tx.id()))
            .cloned()
            .collect();
        self.settle_pool(returning, &confirmed);
    }

    pub(crate) fn announce(&mut self, except: Option<PeerId>) {
        let message = self.tip_message();
        self.outbox.push_back(Outgoing::All { except, message });
    }

    /// What this node announces of its tip.
    pub fn tip_message(&self) -> Message {
        Message::Tip {
            genesis: self.chain.genesis_id(),
            tip: self.chain.tip(),
            previous: self.chain.tip_header().previous,
            height: self.chain.height(),
            work: self.chain.total_work(),
        }
    }

    fn refresh_candidate(&mut self, device_ms: u64) {
        let stale = device_ms.saturating_sub(self.candidate_built_ms) >= REFRESH_TEMPLATE_EVERY_MS;
        let now = self.chain_now(device_ms);
        let retime = self.candidate.as_ref().is_some_and(|block| stale || self.floor_has_come(&block.header, now));
        match self.candidate.as_mut() {
            None => {
                let miner = self.miner();
                let block = self
                    .chain
                    .block_template_with(&miner, &self.pool, now, &self.note);
                self.candidate = Some(block);
                self.candidate_built_ms = device_ms;
            }
            // A tip or pool change clears the candidate, so only its time can be out of date.
            // Building it again would copy the unspent set whenever a payment is waiting.
            Some(_) if retime => {
                self.retime_candidate(now);
                self.candidate_built_ms = device_ms;
            }
            Some(_) => {}
        }
    }

    pub(crate) fn push_event(&mut self, event: Event) {
        if self.events.len() >= MAX_EVENTS {
            self.events.pop_front();
        }
        self.events.push_back(event);
    }
}

/// Notes that `peer` was heard, on a list of a fixed size.
pub(crate) fn hear_peer(peers: &mut BTreeMap<PeerId, u64>, peer: PeerId, now: u64) {
    if peers.len() >= MOST_PEERS && !peers.contains_key(&peer) {
        let oldest = peers.iter().min_by_key(|(_, heard)| **heard).map(|(peer, _)| *peer);
        if let Some(oldest) = oldest {
            peers.remove(&oldest);
        }
    }
    peers.insert(peer, now);
}
