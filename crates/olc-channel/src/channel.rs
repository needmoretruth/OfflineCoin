//! One channel's state and its steps (BOLT 2). States move in turns: one side offers updates, the
//! other signs the new state, the first signs back and revokes its old one, the second revokes
//! too. Crossing offers are settled by node id before anyone signs, so no signature is ever given
//! for a state that is dropped. Nothing here touches a disk or a socket: the caller saves the
//! channel before it sends what a step returns.
use crate::bytes::{Bytes, Key, Signature};
use crate::keys::{derive_secret, state_point, state_secret, Secrets, SecretStore};
use crate::script;
use crate::tx::{closing, commitment, htlc_tx, sign_input, signed_by, CommitKeys, Commitment, Htlc, Terms};
use crate::wire::{ChannelId, Hop, NodeId, Opening, Update, Wire};
use olc_core::{Address, OutPoint, Tx};
use olc_crypto::{public_key, sha256, Hash};
use serde::{Deserialize, Serialize};

/// Most payments waiting in one channel at once. Each is an output a block must find room for.
pub const MOST_HTLCS: usize = 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Phase {
    /// The opener asked and waits for an answer.
    Asked,
    /// The other side said yes and waits for the funding output.
    Accepted,
    /// The opener signed the other's first state and waits for its own.
    FundingSent,
    /// Both first states are signed; the funding transaction waits for blocks.
    Confirming,
    Open,
    /// Both signed the cooperative close.
    Closing,
    /// A commitment went to the chain, and what is ours is being taken.
    ForceClosing,
    Closed,
    /// It never opened.
    Failed,
}

/// A payment waiting on a preimage, as both sides hold it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pending {
    pub id: u64,
    /// Offered by this side.
    pub ours: bool,
    pub amount: u64,
    pub hash: Hash,
    pub expiry: u32,
    pub onward: Vec<Hop>,
}

/// A payment this side offered because one was offered to it on another channel: kept in the same
/// file as the offer, so a restart can neither lose the link nor pass the payment on twice.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Origin {
    /// The id of the payment offered here.
    pub id: u64,
    pub hash: Hash,
    /// The channel and id of the payment it came from.
    pub from: (ChannelId, u64),
}

/// One state of the channel.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct State {
    pub number: u64,
    pub ours: u64,
    pub theirs: u64,
    pub htlcs: Vec<Pending>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Step {
    Offered,
    Signed,
    SignRevoked,
    Closing,
}

/// A turn under way.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Round {
    pub mine: bool,
    pub number: u64,
    pub updates: Vec<Update>,
    pub step: Step,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Channel {
    pub id: ChannelId,
    pub peer: NodeId,
    pub we_opened: bool,
    pub phase: Phase,
    pub seed: Hash,
    pub ours: Opening,
    pub theirs: Option<Opening>,
    pub terms: Terms,
    pub funding_tx: Option<Tx>,
    /// Our latest state the other side signed: what we would close with.
    pub state: State,
    pub signature: Option<Signature>,
    pub htlc_signatures: Vec<Signature>,
    /// Their latest state we signed, and the one before while they have not revoked it.
    pub their_state: State,
    pub their_old: Option<State>,
    /// Their points by state number, the last few.
    pub their_points: Vec<(u64, Key)>,
    pub their_secrets: SecretStore,
    pub round: Option<Round>,
    pub queue: Vec<Update>,
    pub next_id: u64,
    pub last_sent: Option<Wire>,
    pub closing_tx: Option<Tx>,
    /// The height the funding output reached its depth at.
    pub open_height: Option<u64>,
    pub their_announce: Option<Signature>,
    /// Their funding key's signature on the announcement, as BOLT 7 asks beside the node's.
    #[serde(default)]
    pub their_funding_announce: Option<Signature>,
    /// The wallet's coins this channel is funded from, held from the ask until it opens or fails.
    #[serde(default)]
    pub held: Vec<OutPoint>,
    pub made_ms: u64,
    /// When it closed or failed to open, so two ended channels with one computer read apart.
    #[serde(default)]
    pub ended_ms: Option<u64>,
    #[serde(default)]
    pub origins: Vec<Origin>,
    /// What the chain shows once the funding output is spent.
    #[serde(default)]
    pub chain: crate::watch::ChainSeen,
}

pub type Fault = &'static str;

fn wrap(signature: Option<[u8; 64]>) -> Result<Signature, Fault> {
    signature.map(Bytes).ok_or("could not sign")
}

/// Applies `updates` proposed by one side to `state`. Each is checked as the other side checks it.
pub fn apply(state: &State, updates: &[Update], by_us: bool, funder_us: bool, fee: u64, height: u64) -> Result<State, Fault> {
    let mut next = state.clone();
    next.number += 1;
    for update in updates {
        match update {
            Update::Add { id, amount, hash, expiry, onward } => {
                let (balance, funds) = match by_us {
                    true => (&mut next.ours, funder_us),
                    false => (&mut next.theirs, !funder_us),
                };
                let keep = if funds { fee } else { 0 };
                if *amount == 0 || balance.saturating_sub(keep) < *amount {
                    return Err("not enough in the channel");
                }
                if u64::from(*expiry) <= height {
                    return Err("an expiry already past");
                }
                if next.htlcs.len() >= MOST_HTLCS || next.htlcs.iter().any(|h| h.ours == by_us && h.id == *id) {
                    return Err("too many payments waiting");
                }
                *balance -= amount;
                let pending = Pending {
                    id: *id,
                    ours: by_us,
                    amount: *amount,
                    hash: *hash,
                    expiry: *expiry,
                    onward: onward.clone(),
                };
                next.htlcs.push(pending);
            }
            Update::Fulfil { id, preimage } => {
                let at = next.htlcs.iter().position(|h| h.ours != by_us && h.id == *id).ok_or("no such payment")?;
                if sha256(preimage) != next.htlcs[at].hash {
                    return Err("a wrong preimage");
                }
                let paid = next.htlcs.remove(at).amount;
                match by_us {
                    true => next.ours += paid,
                    false => next.theirs += paid,
                }
            }
            Update::Fail { id } => {
                let at = next.htlcs.iter().position(|h| h.ours != by_us && h.id == *id).ok_or("no such payment")?;
                let back = next.htlcs.remove(at).amount;
                match by_us {
                    true => next.theirs += back,
                    false => next.ours += back,
                }
            }
        }
    }
    Ok(next)
}

fn htlcs_of(state: &State, holder_is_us: bool) -> Vec<Htlc> {
    let as_holder = |pending: &Pending| Htlc {
        id: pending.id,
        offered: pending.ours == holder_is_us,
        amount: pending.amount,
        hash: pending.hash,
        expiry: pending.expiry,
    };
    state.htlcs.iter().map(as_holder).collect()
}

impl Channel {
    pub fn secrets(&self) -> Secrets {
        Secrets::from_seed(&self.seed)
    }

    pub fn their_side(&self) -> Result<&Opening, Fault> {
        self.theirs.as_ref().ok_or("the other side has not answered")
    }

    pub fn their_point(&self, number: u64) -> Option<[u8; 33]> {
        self.their_points.iter().find(|(at, _)| *at == number).map(|(_, point)| point.0)
    }

    pub fn our_point(&self, number: u64) -> Option<[u8; 33]> {
        state_point(&self.secrets().states, number)
    }

    pub fn funding_lock(&self) -> Result<Vec<u8>, Fault> {
        Ok(script::funding(&self.ours.keys.funding.0, &self.their_side()?.keys.funding.0))
    }

    /// The keys of our commitment at `number`.
    pub fn our_keys(&self, number: u64) -> Result<CommitKeys, Fault> {
        let point = self.our_point(number).ok_or("no point")?;
        CommitKeys::new(&self.ours.keys, &self.their_side()?.keys, &point).ok_or("bad keys")
    }

    pub fn their_keys(&self, number: u64) -> Result<CommitKeys, Fault> {
        let point = self.their_point(number).ok_or("no point of theirs for that state")?;
        CommitKeys::new(&self.their_side()?.keys, &self.ours.keys, &point).ok_or("bad keys")
    }

    pub fn our_commitment(&self, state: &State) -> Result<Commitment, Fault> {
        let keys = self.our_keys(state.number)?;
        let their_pay_to = self.their_side()?.pay_to;
        let holder = (state.ours, self.we_opened);
        Ok(commitment(&self.terms, &keys, state.number, holder, (state.theirs, &their_pay_to), &htlcs_of(state, true)))
    }

    pub fn their_commitment(&self, state: &State) -> Result<Commitment, Fault> {
        let keys = self.their_keys(state.number)?;
        let holder = (state.theirs, !self.we_opened);
        let other = (state.ours, &self.ours.pay_to);
        Ok(commitment(&self.terms, &keys, state.number, holder, other, &htlcs_of(state, false)))
    }

    /// Our signatures on their commitment for `state` and on its second-stage transactions.
    fn sign_theirs(&self, state: &State) -> Result<(Signature, Vec<Signature>), Fault> {
        let secrets = self.secrets();
        let commit = self.their_commitment(state)?;
        let lock = self.funding_lock()?;
        let signature = wrap(sign_input(&commit.tx, 0, &lock, &self.terms.coin, &secrets.funding))?;
        let point = self.their_point(state.number).ok_or("no point")?;
        let htlc_secret = derive_secret(&secrets.htlc, &point).ok_or("bad key")?;
        let keys = self.their_keys(state.number)?;
        let id = commit.tx.id();
        let mut htlcs = Vec::new();
        for (place, htlc) in &commit.htlcs {
            let second = htlc_tx(&self.terms, &keys, &id, *place, htlc);
            htlcs.push(wrap(sign_input(&second, 0, &keys.htlc_lock(htlc), &self.terms.coin, &htlc_secret))?);
        }
        Ok((signature, htlcs))
    }

    /// Checks their signatures on our commitment for `state`.
    fn check_ours(&self, state: &State, signature: &Signature, htlcs: &[Signature]) -> Result<(), Fault> {
        let commit = self.our_commitment(state)?;
        let lock = self.funding_lock()?;
        let funding_key = self.their_side()?.keys.funding;
        if !signed_by(&commit.tx, 0, &lock, &self.terms.coin, &funding_key.0, &signature.0) {
            return Err("a wrong signature on our state");
        }
        if htlcs.len() != commit.htlcs.len() {
            return Err("wrong number of payment signatures");
        }
        let keys = self.our_keys(state.number)?;
        let id = commit.tx.id();
        for ((place, htlc), signature) in commit.htlcs.iter().zip(htlcs) {
            let second = htlc_tx(&self.terms, &keys, &id, *place, htlc);
            if !signed_by(&second, 0, &keys.htlc_lock(htlc), &self.terms.coin, &keys.remote_htlc, &signature.0) {
                return Err("a wrong payment signature");
            }
        }
        Ok(())
    }

    /// Takes their secret for `number`, checked against the point they gave for it.
    fn take_secret(&mut self, number: u64, secret: &Hash, next: Key) -> Result<(), Fault> {
        if public_key(secret) != self.their_point(number) {
            return Err("a secret that is not theirs");
        }
        if !self.their_secrets.insert(number, *secret) {
            return Err("a secret off their seed");
        }
        self.their_points.retain(|(at, _)| *at > number);
        if self.their_point(number + 2).is_none() {
            self.their_points.push((number + 2, next));
        }
        Ok(())
    }

    fn sent(&mut self, wire: Wire) -> Option<Wire> {
        self.last_sent = Some(wire.clone());
        Some(wire)
    }

    /// Updates waiting, the ones that still fit the state.
    fn fitting(&self, height: u64) -> (Vec<Update>, Vec<Update>) {
        let (mut fit, mut dropped) = (Vec::new(), Vec::new());
        for update in &self.queue {
            let mut tried = fit.clone();
            tried.push(update.clone());
            match apply(&self.state, &tried, true, self.we_opened, self.terms.fee, height) {
                Ok(_) => fit = tried,
                Err(_) => dropped.push(update.clone()),
            }
        }
        (fit, dropped)
    }

    /// Starts a turn with what waits in the queue. Gives the updates it had to drop as well.
    pub fn propose(&mut self, height: u64) -> (Option<Wire>, Vec<Update>) {
        if self.phase != Phase::Open || self.round.is_some() || self.queue.is_empty() {
            return (None, Vec::new());
        }
        let (fit, dropped) = self.fitting(height);
        self.queue.clear();
        if fit.is_empty() {
            return (None, dropped);
        }
        let number = self.state.number + 1;
        self.round = Some(Round { mine: true, number, updates: fit.clone(), step: Step::Offered });
        (self.sent(Wire::Offer { id: self.id, number, updates: fit }), dropped)
    }

    /// Their offer. `we_win` settles crossing offers.
    pub fn on_offer(&mut self, number: u64, updates: Vec<Update>, we_win: bool) -> Result<Option<Wire>, Fault> {
        if self.phase != Phase::Open || number != self.state.number + 1 {
            return Ok(None);
        }
        match &self.round {
            Some(round) if round.mine && matches!(round.step, Step::Offered | Step::Closing) => {
                if we_win {
                    return Ok(None);
                }
                let mut back = round.updates.clone();
                back.append(&mut self.queue);
                self.queue = back;
                self.round = None;
            }
            Some(round) if !round.mine && round.number == number => return Ok(self.last_sent.clone()),
            Some(_) => return Ok(None),
            None => {}
        }
        // Expiries are not held against them here: blocks reach the two sides at different times,
        // and a payment offered too late is given back by the flow instead.
        let next = apply(&self.state, &updates, false, self.we_opened, self.terms.fee, 0)?;
        let (signature, htlcs) = self.sign_theirs(&next)?;
        self.their_old = Some(std::mem::replace(&mut self.their_state, next));
        self.round = Some(Round { mine: false, number, updates, step: Step::Signed });
        Ok(self.sent(Wire::Sign { id: self.id, number, signature, htlcs }))
    }

    /// Their signature on our next state: we sign theirs and revoke our last.
    pub fn on_sign(&mut self, number: u64, signature: Signature, htlcs: Vec<Signature>) -> Result<Option<Wire>, Fault> {
        let Some(round) = self.round.clone().filter(|round| round.mine && round.number == number) else {
            return Ok(None);
        };
        if round.step == Step::SignRevoked {
            return Ok(self.last_sent.clone());
        }
        if round.step != Step::Offered {
            return Ok(None);
        }
        // Checked when it was offered; a block since then changes nothing agreed.
        let next = apply(&self.state, &round.updates, true, self.we_opened, self.terms.fee, 0)?;
        self.check_ours(&next, &signature, &htlcs)?;
        let (theirs, their_htlcs) = self.sign_theirs(&next)?;
        let old = self.state.number;
        let secret = state_secret(&self.secrets().states, old);
        let following = Bytes(self.our_point(number + 1).ok_or("no point")?);
        self.state = next.clone();
        self.signature = Some(signature);
        self.htlc_signatures = htlcs;
        self.their_old = Some(std::mem::replace(&mut self.their_state, next));
        self.round = Some(Round { step: Step::SignRevoked, ..round });
        let wire = Wire::SignRevoke { id: self.id, number, signature: theirs, htlcs: their_htlcs, secret, next: following };
        Ok(self.sent(wire))
    }

    /// Their signature and revocation: our turn as the other side ends with our revocation.
    pub fn on_sign_revoke(
        &mut self,
        number: u64,
        signature: Signature,
        htlcs: Vec<Signature>,
        secret: Hash,
        next: Key,
    ) -> Result<Option<Wire>, Fault> {
        let Some(round) = self.round.clone().filter(|round| !round.mine && round.number == number) else {
            // That turn ended here but our revocation did not arrive. It follows from the seed,
            // so it is made again rather than kept, even after a turn of ours began.
            if number == self.state.number && number > 0 && self.phase == Phase::Open {
                let secret = state_secret(&self.secrets().states, number - 1);
                let following = Bytes(self.our_point(number + 1).ok_or("no point")?);
                return Ok(Some(Wire::Revoke { id: self.id, number, secret, next: following }));
            }
            return Ok(None);
        };
        if round.step != Step::Signed {
            return Ok(None);
        }
        let new = self.their_state.clone();
        self.check_ours(&new, &signature, &htlcs)?;
        self.take_secret(number - 1, &secret, next)?;
        let own = state_secret(&self.secrets().states, number - 1);
        let following = Bytes(self.our_point(number + 1).ok_or("no point")?);
        self.state = new;
        self.signature = Some(signature);
        self.htlc_signatures = htlcs;
        self.their_old = None;
        self.round = None;
        Ok(self.sent(Wire::Revoke { id: self.id, number, secret: own, next: following }))
    }

    /// Their revocation ends our turn.
    pub fn on_revoke(&mut self, number: u64, secret: Hash, next: Key) -> Result<bool, Fault> {
        let ours = self.round.as_ref().is_some_and(|r| r.mine && r.number == number && r.step == Step::SignRevoked);
        if !ours {
            return Ok(false);
        }
        self.take_secret(number - 1, &secret, next)?;
        self.their_old = None;
        self.round = None;
        Ok(true)
    }

    /// Each side's coins in a cooperative close now, the opener paying the fee.
    pub fn closing_shares(&self) -> Result<[(Address, u64); 2], Fault> {
        let fee = self.terms.fee;
        let (mut ours, mut theirs) = (self.state.ours, self.state.theirs);
        match self.we_opened {
            true => ours = ours.checked_sub(fee).ok_or("not enough for the fee")?,
            false => theirs = theirs.checked_sub(fee).ok_or("not enough for the fee")?,
        }
        Ok([(self.ours.pay_to, ours), (self.their_side()?.pay_to, theirs)])
    }

    fn closing_lock_and_tx(&self) -> Result<(Vec<u8>, Tx), Fault> {
        Ok((self.funding_lock()?, closing(&self.terms, self.closing_shares()?)))
    }

    fn finish_closing(&self, mut tx: Tx, theirs: &Signature, ours: &Signature) -> Result<Tx, Fault> {
        let their_key = self.their_side()?.keys.funding;
        let unlock = script::funding_unlock((&self.ours.keys.funding.0, &ours.0), (&their_key.0, &theirs.0));
        tx.inputs[0].unlock = unlock;
        Ok(tx)
    }

    /// Offers the cooperative close, when no payment waits and no turn is under way.
    pub fn propose_close(&mut self) -> Result<Wire, Fault> {
        if self.phase != Phase::Open || self.round.is_some() || !self.state.htlcs.is_empty() || !self.queue.is_empty() {
            return Err("a payment is still under way");
        }
        let (lock, tx) = self.closing_lock_and_tx()?;
        let signature = wrap(sign_input(&tx, 0, &lock, &self.terms.coin, &self.secrets().funding))?;
        let number = self.state.number;
        self.round = Some(Round { mine: true, number, updates: Vec::new(), step: Step::Closing });
        let wire = Wire::Close { id: self.id, number, signature };
        self.last_sent = Some(wire.clone());
        Ok(wire)
    }

    /// Their close offer: signed back and finished, or `None` when it cannot be taken now.
    pub fn on_close(&mut self, number: u64, signature: Signature, we_win: bool) -> Result<Option<(Wire, Tx)>, Fault> {
        if self.phase == Phase::Closing {
            let tx = self.closing_tx.clone().ok_or("no close kept")?;
            return Ok(self.last_sent.clone().map(|wire| (wire, tx)));
        }
        if self.phase != Phase::Open || number != self.state.number || !self.state.htlcs.is_empty() {
            return Ok(None);
        }
        match &self.round {
            Some(round) if round.mine && matches!(round.step, Step::Offered | Step::Closing) && !we_win => {
                let mut back = round.updates.clone();
                back.append(&mut self.queue);
                self.queue = back;
                self.round = None;
            }
            Some(_) => return Ok(None),
            None => {}
        }
        let (lock, tx) = self.closing_lock_and_tx()?;
        let their_key = self.their_side()?.keys.funding;
        if !signed_by(&tx, 0, &lock, &self.terms.coin, &their_key.0, &signature.0) {
            return Err("a wrong signature on the close");
        }
        let ours = wrap(sign_input(&tx, 0, &lock, &self.terms.coin, &self.secrets().funding))?;
        let tx = self.finish_closing(tx, &signature, &ours)?;
        self.phase = Phase::Closing;
        self.closing_tx = Some(tx.clone());
        self.queue.clear();
        let wire = Wire::CloseSigned { id: self.id, signature: ours };
        self.last_sent = Some(wire.clone());
        Ok(Some((wire, tx)))
    }

    pub fn on_close_signed(&mut self, signature: Signature) -> Result<Option<Tx>, Fault> {
        let closing_round = self.round.as_ref().is_some_and(|round| round.mine && round.step == Step::Closing);
        if self.phase != Phase::Open || !closing_round {
            return Ok(None);
        }
        let (lock, tx) = self.closing_lock_and_tx()?;
        let their_key = self.their_side()?.keys.funding;
        if !signed_by(&tx, 0, &lock, &self.terms.coin, &their_key.0, &signature.0) {
            return Err("a wrong signature on the close");
        }
        let ours = wrap(sign_input(&tx, 0, &lock, &self.terms.coin, &self.secrets().funding))?;
        let tx = self.finish_closing(tx, &signature, &ours)?;
        self.phase = Phase::Closing;
        self.closing_tx = Some(tx.clone());
        self.round = None;
        self.queue.clear();
        Ok(Some(tx))
    }

    /// Our latest commitment with both signatures, ready for the chain.
    pub fn signed_commitment(&self) -> Result<Tx, Fault> {
        let signature = self.signature.ok_or("no signature from the other side yet")?;
        let commit = self.our_commitment(&self.state)?;
        let lock = self.funding_lock()?;
        let ours = wrap(sign_input(&commit.tx, 0, &lock, &self.terms.coin, &self.secrets().funding))?;
        self.finish_closing(commit.tx, &signature, &ours)
    }

    /// Payments in flight we offered, in the queue or a turn as well as in the state.
    pub fn offers(&self, id: u64) -> bool {
        let added = |update: &Update| matches!(update, Update::Add { id: added, .. } if *added == id);
        self.state.htlcs.iter().any(|htlc| htlc.ours && htlc.id == id)
            || self.queue.iter().any(added)
            || self.round.as_ref().is_some_and(|round| round.mine && round.updates.iter().any(added))
    }

    /// True when an update for an HTLC offered to us is already on its way.
    pub fn resolving(&self, id: u64) -> bool {
        let about = |update: &Update| match update {
            Update::Fulfil { id: at, .. } | Update::Fail { id: at } => *at == id,
            Update::Add { .. } => false,
        };
        self.queue.iter().any(about) || self.round.as_ref().is_some_and(|round| round.mine && round.updates.iter().any(about))
    }

    /// What we hold in the channel now, payments we offered not counted.
    pub fn balance(&self) -> u64 {
        self.state.ours
    }

    pub fn funding(&self) -> OutPoint {
        self.terms.funding
    }
}
