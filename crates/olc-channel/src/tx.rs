//! The transactions of BOLT 3 on OLC version 2: commitments, the HTLC-success and HTLC-timeout
//! transactions that hang off them, the cooperative close, and spends of what they leave.
use crate::keys::{derive_public, revocation_public, Basepoints};
use crate::script::{self, HtlcKeys};
use olc_core::types::{SEQUENCE_DISABLE, SEQUENCE_FINAL, UNLOCK_FREE_VERSION};
use olc_core::{Address, OutPoint, Tx, TxIn, TxOut, ADDRESS_LANGUAGE};
use olc_crypto::{sign, Hash, PublicKey, SignatureBytes};
use olc_lang::GAMMA;
use serde::{Deserialize, Serialize};

/// What stays fixed for a channel's life.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Terms {
    pub funding: OutPoint,
    pub capacity: u64,
    /// Blocks a side waits for its own coins after it closes alone.
    pub delay: u16,
    /// What each transaction of the channel pays. The coin asks a flat fee.
    pub fee: u64,
    /// Outputs below this are left out and go to the fee.
    pub dust: u64,
    /// Hides the state number in a commitment from everyone else.
    pub obscure: u32,
    /// What signatures name, so a fork or another coin cannot take them.
    pub coin: Hash,
}

/// The keys of one side's commitment at one state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommitKeys {
    pub revocation: PublicKey,
    pub delayed: PublicKey,
    /// The holder's HTLC key.
    pub local_htlc: PublicKey,
    /// The other side's HTLC key.
    pub remote_htlc: PublicKey,
}

impl CommitKeys {
    /// The keys of `holder`'s commitment at the state of `point`, `holder`'s own point.
    pub fn new(holder: &Basepoints, other: &Basepoints, point: &PublicKey) -> Option<Self> {
        Some(Self {
            revocation: revocation_public(&other.revocation.0, point)?,
            delayed: derive_public(&holder.delayed.0, point)?,
            local_htlc: derive_public(&holder.htlc.0, point)?,
            remote_htlc: derive_public(&other.htlc.0, point)?,
        })
    }

    pub fn to_self_lock(&self, delay: u16) -> Vec<u8> {
        script::to_self(&self.revocation, delay, &self.delayed)
    }

    pub fn htlc_lock(&self, htlc: &Htlc) -> Vec<u8> {
        let keys = HtlcKeys { revocation: &self.revocation, remote: &self.remote_htlc, local: &self.local_htlc };
        match htlc.offered {
            true => script::offered_htlc(&keys, &htlc.hash),
            false => script::received_htlc(&keys, &htlc.hash, htlc.expiry),
        }
    }
}

/// A payment that waits on a preimage, from the side of whoever holds the commitment.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Htlc {
    pub id: u64,
    /// Offered by the holder, rather than to it.
    pub offered: bool,
    pub amount: u64,
    pub hash: Hash,
    /// The height from which the offering side may take it back.
    pub expiry: u32,
}

/// A commitment and where its outputs went.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Commitment {
    pub tx: Tx,
    pub to_self: Option<u32>,
    /// HTLCs big enough for an output of their own, with its place.
    pub htlcs: Vec<(u32, Htlc)>,
}

/// The sequence that carries a hidden state number. The top bit keeps it from being a relative
/// lock (BIP 68), so a commitment is final at once.
pub fn hide_number(number: u64, obscure: u32) -> u32 {
    SEQUENCE_DISABLE | ((number as u32 ^ obscure) & !SEQUENCE_DISABLE)
}

pub fn read_number(tx: &Tx, obscure: u32) -> Option<u64> {
    let sequence = tx.inputs.first()?.sequence;
    (sequence & SEQUENCE_DISABLE != 0).then_some(u64::from((sequence ^ obscure) & !SEQUENCE_DISABLE))
}

fn spend_of(point: OutPoint, sequence: u32) -> TxIn {
    TxIn { previous: point, unlock: Vec::new(), sequence }
}

fn gamma(value: u64, lock: Vec<u8>) -> TxOut {
    TxOut { value, language: GAMMA, lock }
}

fn paying(value: u64, address: &Address) -> TxOut {
    TxOut { value, language: ADDRESS_LANGUAGE, lock: address.to_vec() }
}

/// Whether an HTLC is too small to stand on its own once its second transaction pays a fee.
pub fn is_dust(terms: &Terms, amount: u64) -> bool {
    amount < terms.dust.saturating_add(terms.fee)
}

/// The commitment of the holder at state `number`. The funder pays its fee. The other side's
/// share goes straight to its address, as with option_static_remotekey.
pub fn commitment(
    terms: &Terms,
    keys: &CommitKeys,
    number: u64,
    holder: (u64, bool),
    other: (u64, &Address),
    htlcs: &[Htlc],
) -> Commitment {
    let (mut own, holder_funded) = holder;
    let (mut theirs, other_payout) = other;
    match holder_funded {
        true => own = own.saturating_sub(terms.fee),
        false => theirs = theirs.saturating_sub(terms.fee),
    }
    let mut outputs = Vec::new();
    let mut to_self = None;
    if own >= terms.dust {
        to_self = Some(outputs.len() as u32);
        outputs.push(gamma(own, keys.to_self_lock(terms.delay)));
    }
    if theirs >= terms.dust {
        outputs.push(paying(theirs, other_payout));
    }
    let mut sorted: Vec<Htlc> = htlcs.to_vec();
    sorted.sort_by_key(|htlc| (!htlc.offered, htlc.id));
    let mut placed = Vec::new();
    for htlc in sorted.into_iter().filter(|htlc| !is_dust(terms, htlc.amount)) {
        placed.push((outputs.len() as u32, htlc));
        outputs.push(gamma(htlc.amount, keys.htlc_lock(&htlc)));
    }
    let tx = Tx {
        version: UNLOCK_FREE_VERSION,
        inputs: vec![spend_of(terms.funding, hide_number(number, terms.obscure))],
        outputs,
        coinbase_height: None,
        lock_time: 0,
    };
    Commitment { tx, to_self, htlcs: placed }
}

/// The holder's second-stage transaction for an HTLC output: HTLC-timeout for one it offered,
/// usable from the expiry, HTLC-success for one it was offered. Its output waits the delay too.
pub fn htlc_tx(terms: &Terms, keys: &CommitKeys, commitment: &Hash, place: u32, htlc: &Htlc) -> Tx {
    Tx {
        version: UNLOCK_FREE_VERSION,
        inputs: vec![spend_of(OutPoint { txid: *commitment, index: place }, 0)],
        outputs: vec![gamma(htlc.amount.saturating_sub(terms.fee), keys.to_self_lock(terms.delay))],
        coinbase_height: None,
        lock_time: if htlc.offered { htlc.expiry } else { 0 },
    }
}

/// The cooperative close: each side's share to its own address, outputs in address order.
pub fn closing(terms: &Terms, shares: [(Address, u64); 2]) -> Tx {
    let mut shares = shares;
    shares.sort_by_key(|(address, _)| *address);
    let outputs = shares
        .iter()
        .filter(|(_, value)| *value >= terms.dust)
        .map(|(address, value)| paying(*value, address))
        .collect();
    Tx {
        version: UNLOCK_FREE_VERSION,
        inputs: vec![spend_of(terms.funding, SEQUENCE_FINAL)],
        outputs,
        coinbase_height: None,
        lock_time: 0,
    }
}

/// One output to take, with how its input waits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Taking {
    pub point: OutPoint,
    pub value: u64,
    pub sequence: u32,
}

/// Gathers outputs into one payment to `to`, its fee taken from them. `None` when they would not
/// cover it.
pub fn gather(takings: &[Taking], to: &Address, fee: u64, lock_time: u32) -> Option<Tx> {
    let total = takings.iter().map(|taking| taking.value).fold(0u64, u64::saturating_add);
    let value = total.checked_sub(fee).filter(|value| *value > 0)?;
    Some(Tx {
        version: UNLOCK_FREE_VERSION,
        inputs: takings.iter().map(|taking| spend_of(taking.point, taking.sequence)).collect(),
        outputs: vec![paying(value, to)],
        coinbase_height: None,
        lock_time,
    })
}

/// A signature of kind ALL on input `input`, whose coin has `lock`.
pub fn sign_input(tx: &Tx, input: usize, lock: &[u8], coin: &Hash, secret: &[u8; 32]) -> Option<SignatureBytes> {
    sign(secret, &tx.signature_hash(input, lock, Some(coin)))
}

pub fn signed_by(tx: &Tx, input: usize, lock: &[u8], coin: &Hash, key: &PublicKey, signature: &SignatureBytes) -> bool {
    olc_crypto::verify(key, &tx.signature_hash(input, lock, Some(coin)), signature)
}
