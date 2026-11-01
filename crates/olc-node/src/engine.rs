//! Node logic with no I/O: call it with what happened, then collect what it wants to send.
use crate::event::{Event, Outgoing, Status};
use crate::message::Message;
use crate::refused::Refused;
use crate::sync::{Sync, BLOCKS_AT_ONCE};
use alloc::collections::{BTreeMap, BTreeSet, VecDeque};
use alloc::vec::Vec;
use core::cell::Cell;
use olc_core::{
    AddOutcome, Address, Block, Chain, ChainError, CoinConfig, Hash, OutPoint, Plugins, Tx,
};
use olc_wallet::{Wallet, WalletError};

pub type PeerId = u32;

const ANNOUNCE_EVERY_MS: u64 = 2_000;
const REFRESH_TEMPLATE_EVERY_MS: u64 = 5_000;
const MAX_POOL: usize = 256;
const MAX_EVENTS: usize = 32;
/// Own payments remembered, so their change does not read as money received.
const MAX_SENT: usize = 16;
const PEER_TIMEOUT_MS: u64 = 10_000;

pub struct Engine {
    chain: Chain,
    wallet: Wallet,
    pool: Vec<Tx>,
    outbox: VecDeque<Outgoing>,
    events: VecDeque<Event>,
    candidate: Option<Block>,
    candidate_built_ms: u64,
    last_announce_ms: u64,
    /// Added to the device clock. Boards have no real-time clock, so they follow the chain.
    clock_offset_ms: u64,
    now_ms: u64,
    peers: BTreeMap<PeerId, u64>,
    refused: Refused,
    sync: Sync,
    sent: VecDeque<Hash>,
    /// Blocks accepted since the last `take_fresh_block`, for saving.
    fresh: VecDeque<Block>,
    /// The wallet's balance and the tip it was counted on. Counting walks every unspent output
    /// and boards ask for the status many times a second, so it runs once per tip.
    balance: Cell<Option<(Hash, u64)>>,
}

impl Engine {
    pub fn new(config: CoinConfig, plugins: Plugins, wallet: Wallet) -> Self {
        Self {
            chain: Chain::new(config, plugins),
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
            fresh: VecDeque::new(),
            balance: Cell::new(None),
        }
    }

    pub fn chain(&self) -> &Chain {
        &self.chain
    }

    pub fn peer_ids(&self) -> impl Iterator<Item = PeerId> + '_ {
        self.peers.keys().copied()
    }

    pub fn address(&self) -> Address {
        self.wallet.address()
    }

    pub fn status(&self) -> Status {
        Status {
            height: self.chain.height(),
            tip: self.chain.tip(),
            work: self.chain.total_work(),
            difficulty: self.chain.next_difficulty(&self.chain.tip()),
            balance: self.balance(),
            pool: self.pool.len(),
            peers: self.peers.len(),
            syncing_to: self.syncing_to(),
        }
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

    /// True while a transaction waits in this node's pool.
    pub fn in_pool(&self, id: &Hash) -> bool {
        self.pool.iter().any(|tx| tx.id() == *id)
    }

    /// Empties this node's pool, as its owner may. Its coins can be spent anew.
    pub fn clear_pool(&mut self) {
        self.pool.clear();
        self.candidate = None;
    }

    /// Puts a block saved by an earlier run back. Call [`Engine::finish_restoring`] after the last.
    pub fn restore_block(&mut self, block: Block) {
        let _ = self.chain.add_block(block);
    }

    pub fn begin_restoring(&mut self) {
        self.chain.begin_restoring();
    }

    pub fn finish_restoring(&mut self, tick: impl FnMut(usize, usize)) {
        self.chain.finish_restoring(tick);
    }

    /// The next block to save. Call this regularly: the queue holds every new block.
    pub fn take_fresh_block(&mut self) -> Option<Block> {
        self.fresh.pop_front()
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
        self.peers
            .retain(|_, seen| device_ms.saturating_sub(*seen) < PEER_TIMEOUT_MS);
        let chain_ms = self.chain.tip_header().time_ms;
        self.clock_offset_ms = self.clock_offset_ms.max(chain_ms.saturating_sub(device_ms));
        if device_ms.saturating_sub(self.last_announce_ms) >= ANNOUNCE_EVERY_MS {
            self.last_announce_ms = device_ms;
            self.announce(None);
        }
        self.ask_for_blocks();
    }

    pub fn on_message(&mut self, from: PeerId, message: Message) {
        self.peers.insert(from, self.now_ms);
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
                if self.refused.has(&tip) || self.refused.has(&previous) {
                    self.refused.add(tip);
                    return;
                }
                self.sync.heard(from, tip, height, work, self.now_ms);
                self.ask_for_blocks();
            }
            Message::GetBlocks(locator) => {
                for block in self.chain.blocks_after(&locator, BLOCKS_AT_ONCE) {
                    let reply = Message::Block(block.clone());
                    self.outbox.push_back(Outgoing::To(from, reply));
                }
            }
            Message::Block(block) => self.receive_block(from, block),
            Message::Control(_) | Message::Who | Message::Hello(_) => {}
            Message::Tx(tx) => {
                self.receive_tx(Some(from), tx);
            }
        }
    }

    /// The height this node is catching up to, while it is well behind it. It does not mine then.
    pub fn syncing_to(&self) -> Option<u64> {
        let (height, work) = (self.chain.height(), self.chain.total_work());
        self.sync.behind(height, work, self.now_ms)
    }

    fn ask_for_blocks(&mut self) {
        let Some(peer) = self.sync.next_ask(self.chain.total_work(), self.now_ms) else {
            return;
        };
        let mut locator = self.chain.locator();
        if let Some(position) = self.sync.position() {
            locator.insert(0, position);
        }
        self.outbox
            .push_back(Outgoing::To(peer, Message::GetBlocks(locator)));
    }

    /// Runs up to `attempts` sealing attempts and adds the block if one succeeds.
    pub fn mine(&mut self, attempts: u32, device_ms: u64) -> bool {
        if self.syncing_to().is_some() {
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
        if self.chain.add_block(block.clone()).is_err() {
            return false;
        }
        self.fresh.push_back(block);
        self.push_event(Event::Mined { height });
        self.after_tip_change(old_tip);
        self.announce(None);
        true
    }

    /// Signs a payment from this node's wallet and sends it to the network.
    pub fn pay(&mut self, to: Address, amount: u64, fee: u64) -> Result<Hash, WalletError> {
        let reserved: Vec<OutPoint> = self
            .pool
            .iter()
            .flat_map(|tx| tx.inputs.iter().map(|input| input.previous))
            .collect();
        let tx = self.wallet.pay(&self.chain, to, amount, fee, &reserved)?;
        let id = tx.id();
        self.sent.push_back(id);
        if self.sent.len() > MAX_SENT {
            self.sent.pop_front();
        }
        self.push_event(Event::PaymentSent { amount });
        self.receive_tx(None, tx);
        Ok(id)
    }

    /// Hands in a transaction from outside the network, such as a forged test transaction.
    pub fn submit_tx(&mut self, tx: Tx) -> bool {
        self.receive_tx(None, tx)
    }

    fn receive_tx(&mut self, from: Option<PeerId>, tx: Tx) -> bool {
        if tx.is_coinbase() || self.pool.iter().any(|known| known == &tx) {
            return false;
        }
        if self.pool.len() >= MAX_POOL {
            return false;
        }
        match self.chain.check_tx(&tx, self.chain.height() + 1) {
            Ok(_) => {
                self.push_event(Event::TxAccepted);
                self.pool.push(tx.clone());
                self.candidate = None;
                let message = Message::Tx(tx);
                self.outbox.push_back(Outgoing::All {
                    except: from,
                    message,
                });
                true
            }
            Err(reason) => {
                self.push_event(Event::TxRejected { reason });
                false
            }
        }
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
        let copy = block.clone();
        let outcome = self.chain.add_block(block);
        let connected = match outcome {
            Ok(AddOutcome::Orphan { .. }) => None,
            _ => Some(id),
        };
        self.sync.arrived(connected);
        if matches!(
            outcome,
            Ok(AddOutcome::NewTip | AddOutcome::Reorganized { .. } | AddOutcome::SideBranch)
        ) {
            self.fresh.push_back(copy);
        }
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
            Err(ChainError::AlreadyKnown) => {}
            Err(reason) => {
                self.refused.add_invalid(id, reason);
                self.stop_following(from);
                self.push_event(Event::BlockRejected { reason });
            }
        }
    }

    /// A peer whose chain holds an invalid block is no chain to catch up to. It stays a peer.
    fn stop_following(&mut self, peer: PeerId) {
        if let Some(tip) = self.sync.give_up_on(peer) {
            self.refused.add(tip);
        }
    }

    /// What the blocks that joined the best chain paid this wallet, leaving out mining rewards and
    /// the change of its own payments.
    fn received_since(&self, old_tip: &Hash) -> u64 {
        let address = self.wallet.address();
        let (_, joined) = self.chain.branches_from(old_tip);
        joined
            .iter()
            .flat_map(|block| &block.txs)
            .filter(|tx| !tx.is_coinbase() && !self.sent.contains(&tx.id()))
            .flat_map(|tx| &tx.outputs)
            .filter(|output| output.lock.as_slice() == address.as_slice())
            .map(|output| output.value)
            .sum()
    }

    fn after_tip_change(&mut self, old_tip: Hash) {
        self.candidate = None;
        let amount = self.received_since(&old_tip);
        if amount > 0 {
            self.push_event(Event::PaymentReceived { amount });
        }
        let (left, joined) = self.chain.branches_from(&old_tip);
        if left.is_empty() {
            self.drop_spent();
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

    /// Bitcoin's reorganisation rule: payments from the blocks that left go back through the same
    /// check as a new payment, ahead of the pool because a waiting payment may spend their
    /// outputs. Whatever the new chain made impossible is thrown out.
    fn settle_pool(&mut self, returning: Vec<Tx>, confirmed: &BTreeSet<Hash>) {
        let back: BTreeSet<Hash> = returning.iter().map(Tx::id).collect();
        let waiting = core::mem::take(&mut self.pool)
            .into_iter()
            .filter(|tx| !confirmed.contains(&tx.id()) && !back.contains(&tx.id()));
        let (mut kept, refused) = self
            .chain
            .recheck(returning.iter().cloned().chain(waiting).collect());
        let over = kept.split_off(kept.len().min(MAX_POOL));
        for tx in kept.iter().filter(|tx| back.contains(&tx.id())) {
            self.push_event(Event::TxReturned { id: tx.id() });
            self.outbox.push_back(Outgoing::All {
                except: None,
                message: Message::Tx(tx.clone()),
            });
        }
        for (tx, reason) in refused {
            let (id, reason) = (tx.id(), Some(reason));
            self.push_event(Event::TxDropped { id, reason });
        }
        for tx in over {
            let (id, reason) = (tx.id(), None);
            self.push_event(Event::TxDropped { id, reason });
        }
        self.pool = kept;
    }

    /// A waiting payment may spend the output of one before it, as after a reorganisation.
    fn drop_spent(&mut self) {
        let chain = &self.chain;
        let mut made = BTreeSet::new();
        self.pool.retain(|tx| {
            let spendable = tx
                .inputs
                .iter()
                .all(|input| chain.is_unspent(&input.previous) || made.contains(&input.previous));
            if spendable {
                let txid = tx.id();
                made.extend((0..tx.outputs.len() as u32).map(|index| OutPoint { txid, index }));
            }
            spendable
        });
    }

    fn announce(&mut self, except: Option<PeerId>) {
        let message = Message::Tip {
            genesis: self.chain.genesis_id(),
            tip: self.chain.tip(),
            previous: self.chain.tip_header().previous,
            height: self.chain.height(),
            work: self.chain.total_work(),
        };
        self.outbox.push_back(Outgoing::All { except, message });
    }

    fn refresh_candidate(&mut self, device_ms: u64) {
        let stale = device_ms.saturating_sub(self.candidate_built_ms) >= REFRESH_TEMPLATE_EVERY_MS;
        let now = device_ms + self.clock_offset_ms;
        match self.candidate.as_mut() {
            None => {
                let block = self
                    .chain
                    .block_template(&self.wallet.address(), &self.pool, now);
                self.candidate = Some(block);
                self.candidate_built_ms = device_ms;
            }
            // A tip or pool change clears the candidate, so only its time can be out of date.
            // Building it again would copy the unspent set whenever a payment is waiting.
            Some(block) if stale => {
                block.header.time_ms = now.max(block.header.time_ms);
                block.header.nonce = 0;
                self.candidate_built_ms = device_ms;
            }
            Some(_) => {}
        }
    }

    fn push_event(&mut self, event: Event) {
        if self.events.len() >= MAX_EVENTS {
            self.events.pop_front();
        }
        self.events.push_back(event);
    }
}
