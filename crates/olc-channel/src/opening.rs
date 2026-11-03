//! Opening a channel (BOLT 2): open, accept, the funding output with the opener's signature on the
//! other's first state, and the other's signature back. Only then does the opener let the
//! funding transaction go, so neither side can lose what it put in. Version 2 ids leave the
//! unlock data out, so the funding transaction's id is fixed before it is signed for good.
use crate::bytes::{Bytes, Signature};
use crate::channel::{Channel, Fault, Phase, State};
use crate::keys::{state_point, Secrets, SecretStore};
use crate::tx::{sign_input, Terms};
use crate::wire::{ChannelId, NodeId, Opening, Wire};
use olc_core::{Address, OutPoint, Tx};
use olc_crypto::tweak::is_public_key;
use olc_crypto::{sha256, Hash};
use olc_lang::GAMMA;

/// Fewest and most blocks a side may be asked to wait for its own coins after closing alone.
pub const FEWEST_DELAY: u16 = 2;
pub const MOST_DELAY: u16 = 2_016;

/// What the opener asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ask {
    pub capacity: u64,
    pub delay: u16,
    pub fee: u64,
    pub dust: u64,
    pub coin: Hash,
}

fn opening_of(seed: &Hash, payout: Address) -> Result<Opening, Fault> {
    let secrets = Secrets::from_seed(seed);
    let keys = secrets.basepoints().ok_or("bad seed")?;
    let point = |number| state_point(&secrets.states, number).map(Bytes).ok_or("bad seed");
    Ok(Opening { keys, points: [point(0)?, point(1)?], payout })
}

fn sound(side: &Opening) -> bool {
    let keys = [side.keys.funding, side.keys.revocation, side.keys.delayed, side.keys.htlc];
    keys.iter().chain(&side.points).all(|key| is_public_key(&key.0))
}

fn first_state(capacity: u64, we_opened: bool) -> State {
    match we_opened {
        true => State { number: 0, ours: capacity, theirs: 0, htlcs: Vec::new() },
        false => State { number: 0, ours: 0, theirs: capacity, htlcs: Vec::new() },
    }
}

fn blank(id: ChannelId, peer: NodeId, we_opened: bool, ask: &Ask, seed: Hash, ours: Opening, made_ms: u64) -> Channel {
    let state = first_state(ask.capacity, we_opened);
    Channel {
        id,
        peer,
        we_opened,
        phase: Phase::Asked,
        seed,
        ours,
        theirs: None,
        terms: Terms {
            funding: OutPoint { txid: [0; 32], index: 0 },
            capacity: ask.capacity,
            delay: ask.delay,
            fee: ask.fee,
            dust: ask.dust,
            obscure: 0,
            coin: ask.coin,
        },
        funding_tx: None,
        state: state.clone(),
        signature: None,
        htlc_signatures: Vec::new(),
        their_state: state,
        their_old: None,
        their_points: Vec::new(),
        their_secrets: SecretStore::default(),
        round: None,
        queue: Vec::new(),
        next_id: 0,
        last_sent: None,
        closing_tx: None,
        open_height: None,
        their_announce: None,
        made_ms,
        origins: Vec::new(),
        chain: Default::default(),
    }
}

/// Hides state numbers with both funding keys, the opener's first.
fn obscure_of(opener: &[u8], acceptor: &[u8]) -> u32 {
    let mut data = opener.to_vec();
    data.extend_from_slice(acceptor);
    let digest = sha256(&data);
    u32::from_le_bytes([digest[0], digest[1], digest[2], digest[3]])
}

impl Channel {
    /// The opener's first step.
    pub fn start(peer: NodeId, ask: Ask, seed: Hash, payout: Address, made_ms: u64) -> Result<(Self, Wire), Fault> {
        if ask.capacity <= ask.fee.saturating_add(ask.dust) {
            return Err("too little for a channel");
        }
        let id = sha256(&[&seed[..], &peer.0].concat());
        let ours = opening_of(&seed, payout)?;
        let mut channel = blank(id, peer, true, &ask, seed, ours.clone(), made_ms);
        let wire = Wire::Open {
            id,
            capacity: ask.capacity,
            delay: ask.delay,
            fee: ask.fee,
            dust: ask.dust,
            coin: ask.coin,
            side: ours,
        };
        channel.last_sent = Some(wire.clone());
        Ok((channel, wire))
    }

    /// The other side's answer to an open: its keys, or why not. `fee` is what this coin asks.
    pub fn answer(
        id: ChannelId,
        peer: NodeId,
        ask: Ask,
        side: Opening,
        seed: Hash,
        payout: Address,
        made_ms: u64,
    ) -> Result<(Self, Wire), Fault> {
        if ask.capacity <= ask.fee.saturating_add(ask.dust) {
            return Err("too little for a channel");
        }
        if !(FEWEST_DELAY..=MOST_DELAY).contains(&ask.delay) {
            return Err("a wait this node does not take");
        }
        if !sound(&side) {
            return Err("keys that are no keys");
        }
        let ours = opening_of(&seed, payout)?;
        let mut channel = blank(id, peer, false, &ask, seed, ours.clone(), made_ms);
        channel.their_points = vec![(0, side.points[0]), (1, side.points[1])];
        channel.terms.obscure = obscure_of(&side.keys.funding.0, &ours.keys.funding.0);
        channel.theirs = Some(side);
        channel.phase = Phase::Accepted;
        let wire = Wire::Accept { id, side: ours };
        channel.last_sent = Some(wire.clone());
        Ok((channel, wire))
    }

    /// The other side said yes. Gives the lock the funding output must carry.
    pub fn on_accept(&mut self, side: Opening) -> Result<Vec<u8>, Fault> {
        if self.phase != Phase::Asked {
            return Err("not waiting for an answer");
        }
        if !sound(&side) {
            return Err("keys that are no keys");
        }
        self.their_points = vec![(0, side.points[0]), (1, side.points[1])];
        self.terms.obscure = obscure_of(&self.ours.keys.funding.0, &side.keys.funding.0);
        self.theirs = Some(side);
        self.funding_lock()
    }

    /// The opener's funding transaction, signed and kept back: the other's first state gets our
    /// signature now.
    pub fn funded(&mut self, tx: Tx) -> Result<Wire, Fault> {
        let lock = self.funding_lock()?;
        let index = tx
            .outputs
            .iter()
            .position(|out| out.language == GAMMA && out.lock == lock && out.value == self.terms.capacity)
            .ok_or("the funding output is missing")?;
        self.terms.funding = OutPoint { txid: tx.id(), index: index as u32 };
        let theirs = self.their_commitment(&self.their_state)?;
        let secrets = self.secrets();
        let signature = sign_input(&theirs.tx, 0, &lock, &self.terms.coin, &secrets.funding).ok_or("could not sign")?;
        self.funding_tx = Some(tx);
        self.phase = Phase::FundingSent;
        let wire = Wire::Funding { id: self.id, funding: self.terms.funding, signature: Bytes(signature) };
        self.last_sent = Some(wire.clone());
        Ok(wire)
    }

    /// The other side's step: its first state is signed, so it signs the opener's.
    pub fn on_funding(&mut self, funding: OutPoint, signature: Signature) -> Result<Option<Wire>, Fault> {
        if self.phase == Phase::Confirming && self.terms.funding == funding {
            return Ok(self.last_sent.clone());
        }
        if self.phase != Phase::Accepted {
            return Ok(None);
        }
        self.terms.funding = funding;
        let state = self.state.clone();
        self.check_first(&state, &signature)?;
        let theirs = self.their_commitment(&self.their_state)?;
        let lock = self.funding_lock()?;
        let ours = sign_input(&theirs.tx, 0, &lock, &self.terms.coin, &self.secrets().funding).ok_or("could not sign")?;
        self.signature = Some(signature);
        self.phase = Phase::Confirming;
        let wire = Wire::FundingSigned { id: self.id, signature: Bytes(ours) };
        self.last_sent = Some(wire.clone());
        Ok(Some(wire))
    }

    fn check_first(&self, state: &State, signature: &Signature) -> Result<(), Fault> {
        let commit = self.our_commitment(state)?;
        let lock = self.funding_lock()?;
        let key = self.their_side()?.keys.funding;
        match crate::tx::signed_by(&commit.tx, 0, &lock, &self.terms.coin, &key.0, &signature.0) {
            true => Ok(()),
            false => Err("a wrong signature on the first state"),
        }
    }

    /// The opener holds a signed first state, so the funding transaction may go.
    pub fn on_funding_signed(&mut self, signature: Signature) -> Result<Option<Tx>, Fault> {
        if self.phase != Phase::FundingSent {
            return Ok(None);
        }
        let state = self.state.clone();
        self.check_first(&state, &signature)?;
        self.signature = Some(signature);
        self.phase = Phase::Confirming;
        self.last_sent = None;
        Ok(self.funding_tx.clone())
    }
}
