//! The watcher: reads each new block for spends of a channel's funding output and of what came
//! after it, then takes what is ours. A state the other side revoked earns the penalty: every
//! output it left, its own and the payments', goes to us with the revocation key, before its wait
//! runs out. The blocks read so far are saved, and a block that leaves the best chain is read again.
use crate::channel::{Channel, Phase};
use crate::keys::{derive_secret, revocation_secret};
use crate::node::{ChannelNode, Ground, Said};
use crate::script;
use crate::tx::{gather, htlc_tx, read_number, sign_input, CommitKeys, Taking};
use crate::wire::ChannelId;
use olc_core::{OutPoint, Tx, TxOut};
use olc_crypto::{public_key, sha256, Hash};
use olc_lang::gamma::{read_push, signature_checks};
use olc_lang::words::MOST_SIGNATURE_CHECKS;
use serde::{Deserialize, Serialize};

/// Blocks read again after the best chain changed under the last one read.
const REREAD: u64 = 12;
/// Blocks read in one go, so a node far behind still answers between them.
const MOST_PER_TICK: u64 = 64;
/// Blocks read back when a node first watches.
const FIRST_LOOK: u64 = 100;
/// Transactions a channel keeps from the chain.
const MOST_SEEN: usize = 32;

/// How the funding output was spent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Closer {
    Together,
    Ours(u64),
    Theirs(u64),
    Revoked(u64),
    Unknown,
}

/// What the chain shows of a channel.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChainSeen {
    /// The spend of the funding output first, then spends of what it left, with their heights.
    pub seen: Vec<(u64, Tx)>,
    pub closer: Option<Closer>,
    pub penalty: Option<Hash>,
    pub claims: Vec<Hash>,
}

impl ChainSeen {
    pub fn forced_by_us(&self) -> Option<bool> {
        match self.closer? {
            Closer::Ours(_) => Some(true),
            Closer::Theirs(_) | Closer::Revoked(_) => Some(false),
            _ => None,
        }
    }

    pub fn cheated(&self) -> bool {
        matches!(self.closer, Some(Closer::Revoked(_)))
    }

    fn watches(&self, point: &OutPoint, funding: &OutPoint) -> bool {
        *point == *funding || self.seen.iter().any(|(_, tx)| tx.id() == point.txid)
    }
}

/// Builds the words that open an output from the signature over the claim.
type Unlock = Box<dyn Fn(&[u8; 64]) -> Vec<u8>>;

/// One output to take, with how it opens.
struct Claim {
    taking: Taking,
    lock: Vec<u8>,
    secret: [u8; 32],
    /// Builds the unlock from the signature.
    unlock: Unlock,
    lock_time: u32,
}

/// Blocks until our own coins of a commitment we sent can be taken.
pub fn blocks_left(channel: &Channel, height: u64) -> Option<u64> {
    let (at, _) = channel.chain.seen.first()?;
    match channel.chain.closer? {
        Closer::Ours(_) if channel.phase == Phase::ForceClosing => {
            Some((at + u64::from(channel.terms.delay)).saturating_sub(height + 1))
        }
        _ => None,
    }
}

fn classify(channel: &Channel, close: &Tx) -> Closer {
    let id = close.id();
    if channel.closing_tx.as_ref().is_some_and(|tx| tx.id() == id) {
        return Closer::Together;
    }
    let Some(number) = read_number(close, channel.terms.obscure) else {
        return Closer::Unknown;
    };
    if number == channel.state.number && channel.our_commitment(&channel.state).is_ok_and(|ours| ours.tx.id() == id) {
        return Closer::Ours(number);
    }
    let theirs = [Some(&channel.their_state), channel.their_old.as_ref()];
    for state in theirs.into_iter().flatten() {
        if state.number == number && channel.their_commitment(state).is_ok_and(|theirs| theirs.tx.id() == id) {
            return Closer::Theirs(number);
        }
    }
    match channel.their_secrets.get(number) {
        Some(_) => Closer::Revoked(number),
        None => Closer::Unknown,
    }
}

/// 32-byte values in the unlocks of `tx`, which may be preimages.
fn preimages_in(tx: &Tx) -> Vec<Hash> {
    let mut found = Vec::new();
    for input in &tx.inputs {
        let mut at = 0;
        while let Some(code) = input.unlock.get(at).copied() {
            at += 1;
            let Some((data, next)) = read_push(&input.unlock, at, code) else {
                break;
            };
            at = next;
            if let Ok(value) = <[u8; 32]>::try_from(data) {
                found.push(value);
            }
        }
    }
    found
}

/// Outputs of `tx` still unspent, with their place.
fn unspent(tx: &Tx, ground: &dyn Ground) -> Vec<(OutPoint, TxOut, u64)> {
    let id = tx.id();
    (0..tx.outputs.len())
        .filter_map(|index| {
            let point = OutPoint { txid: id, index: u32::try_from(index).ok()? };
            ground.coin(&point).map(|(out, at)| (point, out, at))
        })
        .collect()
}

fn penalty_claims(channel: &Channel, number: u64, ground: &dyn Ground) -> Vec<Claim> {
    let (Some(secret), Ok(theirs)) = (channel.their_secrets.get(number), channel.their_side()) else {
        return Vec::new();
    };
    let ours = channel.secrets();
    let (Some(point), Some(revoking)) = (public_key(&secret), revocation_secret(&ours.revocation, &secret)) else {
        return Vec::new();
    };
    let Some(keys) = CommitKeys::new(&theirs.keys, &channel.ours.keys, &point) else {
        return Vec::new();
    };
    let to_self = keys.to_self_lock(channel.terms.delay);
    let htlc_start = script::htlc_start(&keys.revocation);
    let revocation = keys.revocation;
    let mut claims = Vec::new();
    for (_, tx) in &channel.chain.seen {
        for (point, out, _) in unspent(tx, ground) {
            let unlock: Unlock = if out.lock == to_self {
                Box::new(|signature: &[u8; 64]| script::to_self_by_revocation(signature))
            } else if out.lock.starts_with(&htlc_start) {
                Box::new(move |signature: &[u8; 64]| script::htlc_by_revocation(signature, &revocation))
            } else {
                continue;
            };
            let taking = Taking { point, value: out.value, sequence: 0 };
            claims.push(Claim { taking, lock: out.lock, secret: revoking, unlock, lock_time: 0 });
        }
    }
    claims
}

fn own_claims(channel: &Channel, close: &Tx, preimages: &[(Hash, Hash)], height: u64, ground: &dyn Ground) -> Vec<(Claim, Option<Tx>)> {
    let state = &channel.state;
    let (Ok(commit), Ok(keys), Some(point)) =
        (channel.our_commitment(state), channel.our_keys(state.number), channel.our_point(state.number))
    else {
        return Vec::new();
    };
    let secrets = channel.secrets();
    let (Some(delayed), Some(htlc_secret)) = (derive_secret(&secrets.delayed, &point), derive_secret(&secrets.htlc, &point)) else {
        return Vec::new();
    };
    let delay = channel.terms.delay;
    let to_self = keys.to_self_lock(delay);
    let mut claims = Vec::new();
    // Our own coins, and second-stage outputs, once the wait is over.
    for (_, tx) in &channel.chain.seen {
        for (point, out, at) in unspent(tx, ground) {
            if out.lock == to_self && height + 1 >= at + u64::from(delay) {
                let taking = Taking { point, value: out.value, sequence: u32::from(delay) };
                let unlock = Box::new(|signature: &[u8; 64]| script::to_self_after_delay(signature));
                claims.push((Claim { taking, lock: out.lock, secret: delayed, unlock, lock_time: 0 }, None));
            }
        }
    }
    // Payments on our commitment: back after the expiry, or taken with the preimage.
    let id = close.id();
    for ((place, htlc), theirs) in commit.htlcs.iter().zip(&channel.htlc_signatures) {
        let point = OutPoint { txid: id, index: *place };
        if ground.coin(&point).is_none() {
            continue;
        }
        let preimage = preimages.iter().find(|(hash, _)| *hash == htlc.hash).map(|(_, preimage)| *preimage);
        let ready = match htlc.offered {
            true => height >= u64::from(htlc.expiry),
            false => preimage.is_some(),
        };
        if !ready {
            continue;
        }
        let second = htlc_tx(&channel.terms, &keys, &id, *place, htlc);
        let lock = keys.htlc_lock(htlc);
        let theirs = theirs.0;
        let unlock = Box::new(move |ours: &[u8; 64]| script::htlc_by_both(&theirs, ours, preimage.as_ref()));
        let taking = Taking { point, value: htlc.amount, sequence: 0 };
        claims.push((Claim { taking, lock, secret: htlc_secret, unlock, lock_time: second.lock_time }, Some(second)));
    }
    claims
}

fn their_claims(channel: &Channel, close: &Tx, number: u64, preimages: &[(Hash, Hash)], height: u64, ground: &dyn Ground) -> Vec<Claim> {
    let theirs = [Some(&channel.their_state), channel.their_old.as_ref()];
    let Some(state) = theirs.into_iter().flatten().find(|state| state.number == number) else {
        return Vec::new();
    };
    let (Ok(commit), Ok(keys), Some(point)) = (channel.their_commitment(state), channel.their_keys(number), channel.their_point(number)) else {
        return Vec::new();
    };
    let Some(secret) = derive_secret(&channel.secrets().htlc, &point) else {
        return Vec::new();
    };
    let id = close.id();
    let mut claims = Vec::new();
    for (place, htlc) in &commit.htlcs {
        let point = OutPoint { txid: id, index: *place };
        if ground.coin(&point).is_none() {
            continue;
        }
        let preimage = preimages.iter().find(|(hash, _)| *hash == htlc.hash).map(|(_, preimage)| *preimage);
        // Offered by them is offered to us: ours with the preimage. Theirs to take is ours back
        // after the expiry.
        let ready = match htlc.offered {
            true => preimage.is_some(),
            false => height >= u64::from(htlc.expiry),
        };
        if !ready {
            continue;
        }
        let unlock = Box::new(move |signature: &[u8; 64]| script::htlc_by_remote(signature, preimage.as_ref()));
        let taking = Taking { point, value: htlc.amount, sequence: 0 };
        claims.push(Claim { taking, lock: keys.htlc_lock(htlc), secret, unlock, lock_time: 0 });
    }
    claims
}

/// Signs claims into payments to `channel`'s address, as few as the limit on checks allows.
fn sign_claims(channel: &Channel, claims: Vec<Claim>, fee: u64) -> Vec<Tx> {
    let mut groups: Vec<Vec<Claim>> = Vec::new();
    let mut checks = 0usize;
    for claim in claims {
        let cost = signature_checks(&claim.lock) + 1;
        let fits = groups.last().is_some_and(|group| group.first().is_some_and(|first| first.lock_time == claim.lock_time));
        if !fits || checks + cost > MOST_SIGNATURE_CHECKS {
            groups.push(Vec::new());
            checks = 0;
        }
        checks += cost;
        if let Some(group) = groups.last_mut() {
            group.push(claim);
        }
    }
    let mut signed = Vec::new();
    for group in groups {
        let takings: Vec<Taking> = group.iter().map(|claim| claim.taking.clone()).collect();
        let lock_time = group.first().map_or(0, |claim| claim.lock_time);
        let Some(mut tx) = gather(&takings, &channel.ours.pay_to, fee, lock_time) else {
            continue;
        };
        let unlocks: Option<Vec<Vec<u8>>> = group
            .iter()
            .enumerate()
            .map(|(input, claim)| sign_input(&tx, input, &claim.lock, &channel.terms.coin, &claim.secret).map(|sig| (claim.unlock)(&sig)))
            .collect();
        let Some(unlocks) = unlocks else {
            continue;
        };
        for (input, unlock) in tx.inputs.iter_mut().zip(unlocks) {
            input.unlock = unlock;
        }
        signed.push(tx);
    }
    signed
}

fn watched(channel: &Channel) -> bool {
    matches!(channel.phase, Phase::Confirming | Phase::Open | Phase::Closing | Phase::ForceClosing)
}

impl ChannelNode {
    /// Reads new blocks, then acts on every channel whose funding output is spent.
    pub(crate) fn watch(&mut self, ground: &mut dyn Ground) {
        self.read_blocks(ground);
        let closed: Vec<ChannelId> = self
            .channels
            .values()
            .filter(|channel| !channel.chain.seen.is_empty() && channel.phase != Phase::Closed)
            .map(|channel| channel.id)
            .collect();
        for id in closed {
            self.act(&id, ground);
        }
    }

    /// The first block the watcher may still have to read, while a channel needs watching. The
    /// node keeps it and every block after it, pruned or not, so a cheat in one is never missed.
    pub fn keep_blocks_from(&self) -> Option<u64> {
        let (height, _) = self.book.scanned?;
        let watching = self.channels.values().any(watched);
        // A branch that changes takes the watcher back this far, so those blocks stay too.
        watching.then(|| (height + 1).saturating_sub(REREAD))
    }

    fn read_blocks(&mut self, ground: &mut dyn Ground) {
        let tip = ground.height();
        let read_before = self.book.scanned;
        let mut next = match self.book.scanned {
            Some((height, id)) if ground.block_at(height).is_some_and(|(at, _)| at == id) => height + 1,
            Some((height, _)) => {
                let back = height.saturating_sub(REREAD);
                for channel in self.channels.values_mut() {
                    channel.chain.seen.retain(|(at, _)| *at <= back);
                    if channel.chain.seen.is_empty() {
                        channel.chain.closer = None;
                    }
                }
                back + 1
            }
            None => tip.saturating_sub(FIRST_LOOK),
        };
        let last = tip.min(next + MOST_PER_TICK);
        let mut changed: Vec<ChannelId> = Vec::new();
        while next <= last {
            let Some((block, txs)) = ground.block_at(next) else {
                // The first look starts where blocks are kept; after that every block was held.
                if read_before.is_none() && next < tip {
                    next += 1;
                    continue;
                }
                break;
            };
            for tx in &txs {
                for channel in self.channels.values_mut().filter(|channel| watched(channel)) {
                    let funding = channel.funding();
                    let spends = tx.inputs.iter().any(|input| channel.chain.watches(&input.previous, &funding));
                    let known = channel.chain.seen.iter().any(|(_, seen)| seen.id() == tx.id());
                    if spends && !known && channel.chain.seen.len() < MOST_SEEN {
                        channel.chain.seen.push((next, tx.clone()));
                        changed.push(channel.id);
                    }
                }
            }
            self.book.scanned = Some((next, block));
            next += 1;
        }
        for id in changed {
            if let Some(channel) = self.channels.get(&id).cloned() {
                self.keep(None, channel, None);
            }
        }
        // Written only when the watcher moved, not at every turn of the node.
        if self.book.scanned != read_before {
            self.save_book();
        }
    }

    fn act(&mut self, id: &ChannelId, ground: &mut dyn Ground) {
        let Some(before) = self.channels.get(id).cloned() else {
            return;
        };
        let mut channel = before.clone();
        let Some((_, close)) = channel.chain.seen.first().cloned() else {
            return;
        };
        let closer = *channel.chain.closer.get_or_insert_with(|| classify(&before, &close));
        // Preimages shown on the chain pay what we passed on.
        let hashes: Vec<Hash> = channel.state.htlcs.iter().map(|htlc| htlc.hash).collect();
        let mut learned = false;
        for (_, tx) in &channel.chain.seen {
            for preimage in preimages_in(tx) {
                let hash = sha256(&preimage);
                if hashes.contains(&hash) && !self.book.preimages.iter().any(|(known, _)| *known == hash) {
                    self.book.preimages.push((hash, preimage));
                    learned = true;
                }
            }
        }
        if learned {
            self.save_book();
        }
        let height = ground.height();
        let fee = channel.terms.fee;
        let mut txs: Vec<Tx> = Vec::new();
        let mut penalty = false;
        match closer {
            Closer::Together | Closer::Unknown => {}
            Closer::Ours(_) => {
                for (claim, second) in own_claims(&channel, &close, &self.book.preimages, height, ground) {
                    match second {
                        Some(mut second) => {
                            if let Some(signature) = sign_input(&second, 0, &claim.lock, &channel.terms.coin, &claim.secret) {
                                second.inputs[0].unlock = (claim.unlock)(&signature);
                                txs.push(second);
                            }
                        }
                        None => txs.extend(sign_claims(&channel, vec![claim], fee)),
                    }
                }
            }
            Closer::Theirs(number) => {
                txs = sign_claims(&channel, their_claims(&channel, &close, number, &self.book.preimages, height, ground), fee);
            }
            Closer::Revoked(number) => {
                txs = sign_claims(&channel, penalty_claims(&channel, number, ground), fee);
                penalty = !txs.is_empty();
            }
        }
        let first_penalty = penalty && channel.chain.penalty.is_none();
        for tx in &txs {
            let txid = tx.id();
            if penalty && channel.chain.penalty.is_none() {
                channel.chain.penalty = Some(txid);
            } else if !channel.chain.claims.contains(&txid) && channel.chain.penalty != Some(txid) {
                channel.chain.claims.push(txid);
            }
        }
        let left = txs.is_empty() && self.nothing_left(&channel, closer, ground);
        channel.phase = match (closer, left) {
            (Closer::Together | Closer::Unknown, _) | (_, true) => Phase::Closed,
            _ => Phase::ForceClosing,
        };
        channel.round = None;
        channel.queue.clear();
        channel.last_sent = None;
        let ended = channel.phase == Phase::Closed && before.phase != Phase::Closed;
        if channel != before {
            self.keep(Some(before), channel.clone(), None);
        }
        for tx in &txs {
            self.publish_now_and_then(tx, ground);
        }
        if first_penalty {
            if let Some(txid) = channel.chain.penalty {
                self.say(Said::Penalty { channel: *id, txid });
            }
        }
        if ended {
            self.say(Said::Closed(*id));
        }
    }

    /// True once nothing of ours waits on the chain: no output we could still take stands.
    fn nothing_left(&self, channel: &Channel, closer: Closer, ground: &dyn Ground) -> bool {
        let Some((_, close)) = channel.chain.seen.first() else {
            return false;
        };
        let stands = |place: &u32| ground.coin(&OutPoint { txid: close.id(), index: *place }).is_some();
        match closer {
            Closer::Together | Closer::Unknown => true,
            Closer::Revoked(number) => penalty_claims(channel, number, ground).is_empty(),
            Closer::Theirs(number) => {
                let theirs = [Some(&channel.their_state), channel.their_old.as_ref()];
                let state = theirs.into_iter().flatten().find(|state| state.number == number);
                let commit = state.and_then(|state| channel.their_commitment(state).ok());
                commit.is_none_or(|commit| !commit.htlcs.iter().any(|(place, _)| stands(place)))
            }
            Closer::Ours(_) => {
                let Ok(commit) = channel.our_commitment(&channel.state) else {
                    return true;
                };
                let Ok(keys) = channel.our_keys(channel.state.number) else {
                    return true;
                };
                let to_self = keys.to_self_lock(channel.terms.delay);
                let delayed_stands = channel.chain.seen.iter().any(|(_, tx)| unspent(tx, ground).iter().any(|(_, out, _)| out.lock == to_self));
                !delayed_stands && !commit.htlcs.iter().any(|(place, _)| stands(place))
            }
        }
    }
}
