//! A node that holds headers and no blocks, as in section 8 of the Bitcoin paper. It checks proof
//! of work and follows the most work, and asks full nodes for its coins with Merkle branches.
//!
//! A proof shows a coin is in a block. It cannot show the coin is still unspent or that no coin
//! is missing, so those two rest on the peer that answered.
use crate::engine::PeerId;
use crate::event::Outgoing;
use crate::message::Message;
use crate::sync::Sync;
use alloc::collections::{BTreeMap, VecDeque};
use alloc::vec::Vec;
use olc_core::proof::Coins;
use olc_core::{
    Address, BlockHeader, Chain, CoinProof, HeaderChain, Offered, OutPoint, Tx, Unspent,
};
use olc_wallet::Wallet;

const PEER_TIMEOUT_MS: u64 = 10_000;
const ASK_AGAIN_MS: u64 = 5_000;
/// What a board can hold in PSRAM, at most: about 6 MB of headers, or 40 days at 30 seconds.
pub const MOST_HEADERS_ON_A_BOARD: usize = 64_000;

/// One round of asking a peer for all the coins of this node's addresses.
struct Round {
    peer: PeerId,
    left: Vec<Address>,
    found: BTreeMap<OutPoint, Unspent>,
    asked_ms: u64,
}

/// What a light node can say about itself.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LightStatus {
    pub held_headers: usize,
    pub coins: usize,
    /// Proofs and headers that did not pass.
    pub refused: u32,
    /// Reorganisations followed.
    pub reorganized: u32,
    /// The first block of a peer that dropped blocks this node asked about.
    pub pruned_from: Option<u64>,
    /// Coins too old for the headers still held. They stay counted on the peer's word.
    pub unproven: usize,
}

pub struct Light {
    headers: HeaderChain,
    sync: Sync,
    peers: BTreeMap<PeerId, u64>,
    newest_peer: Option<PeerId>,
    coins: BTreeMap<OutPoint, Unspent>,
    /// Spent by this node, until the coins come back without them.
    spent: Vec<OutPoint>,
    round: Option<Round>,
    /// The coins may have changed since they were last asked for.
    stale: bool,
    quiet_until_ms: u64,
    status: LightStatus,
    outbox: VecDeque<Outgoing>,
    now_ms: u64,
    maturity: u64,
}

impl Coins for Light {
    fn height(&self) -> u64 {
        self.headers.height()
    }

    fn unspent_of(&self, address: &Address) -> Vec<(OutPoint, Unspent)> {
        let mine = |(_, coin): &(&OutPoint, &Unspent)| coin.output.lock.as_slice() == address;
        let free = |(point, _): &(&OutPoint, &Unspent)| !self.spent.contains(point);
        let held = self.coins.iter().filter(mine).filter(free);
        held.map(|(point, coin)| (*point, coin.clone())).collect()
    }

    fn is_mature(&self, entry: &Unspent, spend_height: u64) -> bool {
        !entry.coinbase || spend_height >= entry.height + self.maturity
    }
}

impl Light {
    pub fn new(chain: &Chain, most_headers: usize) -> Self {
        Self {
            headers: HeaderChain::new(chain.config().genesis().header, most_headers),
            sync: Sync::default(),
            peers: BTreeMap::new(),
            newest_peer: None,
            coins: BTreeMap::new(),
            spent: Vec::new(),
            round: None,
            stale: true,
            quiet_until_ms: 0,
            status: LightStatus::default(),
            outbox: VecDeque::new(),
            now_ms: 0,
            maturity: chain.rules().coinbase_maturity,
        }
    }

    pub fn headers(&self) -> &HeaderChain {
        &self.headers
    }

    pub fn status(&self) -> LightStatus {
        LightStatus {
            held_headers: self.headers.held(),
            coins: self.coins.len(),
            ..self.status
        }
    }

    pub fn peers(&self) -> usize {
        self.peers.len()
    }

    pub fn syncing_to(&self) -> Option<u64> {
        self.sync.behind(self.headers.height(), self.headers.work(), self.now_ms)
    }

    /// Asks for the coins again at the next chance.
    pub fn refresh(&mut self) {
        self.stale = true;
    }

    /// Headers saved earlier. Each is checked again, so a damaged file costs only what follows.
    pub fn restore(&mut self, saved: &[BlockHeader], chain: &Chain) {
        let rules = chain.rules();
        let consensus = chain.plugins().consensus.as_ref();
        let _ = self.headers.offer(saved, rules, consensus);
    }

    pub fn take_outgoing(&mut self) -> Option<Outgoing> {
        self.outbox.pop_front()
    }

    /// Sets the coins of a transaction aside. They stay aside until peers stop listing them.
    pub fn spending(&mut self, tx: &Tx) {
        self.spent.extend(tx.inputs.iter().map(|input| input.previous));
    }

    pub fn tick(&mut self, now_ms: u64, wallet: &Wallet, watched: &[Address]) {
        self.now_ms = now_ms;
        self.peers.retain(|_, seen| now_ms.saturating_sub(*seen) < PEER_TIMEOUT_MS);
        if let Some(peer) = self.sync.next_ask(self.headers.work(), now_ms) {
            let mut named = self.headers.locator();
            named.splice(0..0, self.sync.position().or(self.headers.position()));
            self.outbox.push_back(Outgoing::To(peer, Message::GetHeaders(named)));
        }
        let late = |round: &Round| now_ms.saturating_sub(round.asked_ms) >= ASK_AGAIN_MS;
        if self.round.as_ref().is_some_and(late) {
            self.round = None;
            self.stale = true;
        }
        let settled = self.syncing_to().is_none() && now_ms >= self.quiet_until_ms;
        if self.stale && self.round.is_none() && settled {
            self.start_round(wallet, watched);
        }
    }

    fn start_round(&mut self, wallet: &Wallet, watched: &[Address]) {
        let Some(peer) = self.newest_peer.filter(|peer| self.peers.contains_key(peer)) else {
            return;
        };
        let mut left: Vec<Address> = wallet.keys().map(|(address, _)| address).collect();
        left.extend_from_slice(watched);
        self.stale = false;
        self.round = Some(Round {
            peer,
            left,
            found: BTreeMap::new(),
            asked_ms: self.now_ms,
        });
        self.ask_next(None);
    }

    fn ask_next(&mut self, after: Option<OutPoint>) {
        let Some(round) = self.round.as_mut() else {
            return;
        };
        round.asked_ms = self.now_ms;
        match round.left.last() {
            Some(address) => {
                let message = Message::GetCoins { address: *address, after };
                self.outbox.push_back(Outgoing::To(round.peer, message));
            }
            None => self.finish_round(),
        }
    }

    fn finish_round(&mut self) {
        let Some(round) = self.round.take() else {
            return;
        };
        self.coins = round.found;
        self.spent.retain(|point| self.coins.contains_key(point));
    }

    pub fn on_message(&mut self, from: PeerId, message: Message, chain: &Chain) {
        self.peers.insert(from, self.now_ms);
        match message {
            Message::Tip { genesis, tip, height, work, .. } if genesis == chain.genesis_id() => {
                self.sync.heard(from, tip, height, work, self.now_ms);
                if work >= self.headers.work() {
                    self.newest_peer = Some(from);
                }
            }
            // Only a block on the tip says anything. Anything else comes through the headers.
            Message::Block(block) if block.header.previous == self.headers.tip().id() => {
                self.take_headers(&[block.header], chain);
            }
            Message::Headers(batch) => {
                if self.take_headers(&batch, chain) {
                    self.sync.answered(batch.len(), self.headers.position());
                } else {
                    self.sync.give_up_on(from);
                }
            }
            Message::Pruned { oldest } => {
                self.status.pruned_from = Some(oldest);
                self.sync.drop_peer(from);
                self.give_up_round();
            }
            Message::Coin(proof) => self.take_coin(from, &proof),
            Message::CoinsEnd { address, last, more } => self.page_ended(from, address, last, more),
            _ => {}
        }
    }

    fn take_headers(&mut self, batch: &[BlockHeader], chain: &Chain) -> bool {
        let consensus = chain.plugins().consensus.as_ref();
        match self.headers.offer(batch, chain.rules(), consensus) {
            Ok(Offered::Reorganized { .. }) => {
                self.status.reorganized += 1;
                self.round = None;
                self.stale = true;
                true
            }
            Ok(Offered::Extended) => {
                self.stale = true;
                true
            }
            Ok(_) => true,
            Err(_) => {
                self.status.refused += 1;
                false
            }
        }
    }

    fn take_coin(&mut self, from: PeerId, proof: &CoinProof) {
        let Some(round) = self.round.as_mut().filter(|round| round.peer == from) else {
            return;
        };
        let Some(address) = round.left.last() else {
            return;
        };
        let Some(header) = self.headers.header_at(proof.height) else {
            // Headers this far back are gone. What was proven once is kept, on the peer's word.
            if proof.height < self.headers.height() {
                let point = proof.point();
                if let Some(coin) = self.coins.get(&point) {
                    round.found.insert(point, coin.clone());
                    self.status.unproven += 1;
                }
            }
            return;
        };
        match proof.check(header, address) {
            Some((point, coin)) => drop(round.found.insert(point, coin)),
            None => {
                self.status.refused += 1;
                self.give_up_round();
            }
        }
    }

    /// Tries again later: a peer on another branch needs a moment to settle.
    fn give_up_round(&mut self) {
        self.round = None;
        self.stale = true;
        self.quiet_until_ms = self.now_ms + ASK_AGAIN_MS;
    }

    fn page_ended(
        &mut self,
        from: PeerId,
        address: Address,
        last: Option<OutPoint>,
        more: bool,
    ) {
        let Some(round) = self.round.as_mut().filter(|round| round.peer == from) else {
            return;
        };
        if round.left.last() != Some(&address) {
            return;
        }
        if !more {
            round.left.pop();
        }
        self.ask_next(if more { last } else { None });
    }
}
