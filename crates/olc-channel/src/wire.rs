//! What channel nodes say to each other: BOLT 2's steps and BOLT 7's gossip in OLC's own shapes,
//! inside one signed envelope that rides the node's existing links.
use crate::keys::Basepoints;
use olc_core::{Address, OutPoint};
use crate::bytes::{Bytes, Key, Signature};
use olc_crypto::{hash, sign, verify, Hash, SignatureBytes};
use serde::{Deserialize, Serialize};

/// A node's public key, which it signs everything with.
pub type NodeId = Key;
pub type ChannelId = Hash;

/// One step of the route a payment still has to go.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hop {
    /// The channel to pass it on through, by its funding output.
    pub channel: OutPoint,
    pub amount: u64,
    pub expiry: u32,
}

/// A change to a channel's state, proposed by one side.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Update {
    /// Offers a payment that waits on the preimage of `hash` (BOLT 2, update_add_htlc). The route
    /// after this hop rides along in the clear: OLC has no onion routing (BOLT 4) yet.
    Add { id: u64, amount: u64, hash: Hash, expiry: u32, onward: Vec<Hop> },
    /// Takes a payment offered to this side with its preimage (update_fulfill_htlc).
    Fulfil { id: u64, preimage: Hash },
    /// Gives a payment offered to this side back (update_fail_htlc).
    Fail { id: u64 },
}

/// What a side shows when a channel opens.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Opening {
    pub keys: Basepoints,
    /// Its points for states 0 and 1.
    pub points: [Key; 2],
    /// Where its coins go when the channel closes.
    pub payout: Address,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Wire {
    /// Says who this node is to whoever is in reach, and what it is called.
    Hello { name: String },
    /// BOLT 2, open_channel. The opener puts in all of `capacity`.
    Open { id: ChannelId, capacity: u64, delay: u16, fee: u64, dust: u64, coin: Hash, side: Opening },
    Accept { id: ChannelId, side: Opening },
    Refuse { id: ChannelId, why: String },
    /// funding_created: the funding output, and the opener's signature on the other's first state.
    Funding { id: ChannelId, funding: OutPoint, signature: Signature },
    /// funding_signed.
    FundingSigned { id: ChannelId, signature: Signature },
    /// Proposes the next state: the updates only. Signatures follow once both agree whose turn it is.
    Offer { id: ChannelId, number: u64, updates: Vec<Update> },
    /// commitment_signed for the proposer's next state.
    Sign { id: ChannelId, number: u64, signature: Signature, htlcs: Vec<Signature> },
    /// The proposer signs the other's next state and revokes its own last one (revoke_and_ack).
    SignRevoke { id: ChannelId, number: u64, signature: Signature, htlcs: Vec<Signature>, secret: Hash, next: Key },
    /// The other side revokes its last state too, and the step is done.
    Revoke { id: ChannelId, number: u64, secret: Hash, next: Key },
    /// closing_signed: a signature on the cooperative close of the channel as it stands.
    Close { id: ChannelId, number: u64, signature: Signature },
    CloseSigned { id: ChannelId, signature: Signature },
    /// A side's signature on the channel's announcement (announcement_signatures).
    AnnounceSigned { id: ChannelId, signature: Signature },
    Announce(Announcement),
    Terms(ChannelTerms),
}

/// BOLT 7, channel_announcement: both nodes vouch for a funding output on the chain.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Announcement {
    pub funding: OutPoint,
    pub capacity: u64,
    /// Smaller first.
    pub nodes: [NodeId; 2],
    pub funding_keys: [Key; 2],
    pub signatures: [Signature; 2],
}

impl Announcement {
    pub fn digest(funding: &OutPoint, capacity: u64, nodes: &[NodeId; 2], keys: &[Key; 2]) -> Hash {
        let mut data = b"olc channel".to_vec();
        data.extend_from_slice(&funding.txid);
        data.extend_from_slice(&funding.index.to_le_bytes());
        data.extend_from_slice(&capacity.to_le_bytes());
        nodes.iter().chain(keys).for_each(|key| data.extend_from_slice(&key.0));
        hash(&data)
    }

    pub fn signed(&self) -> bool {
        let digest = Self::digest(&self.funding, self.capacity, &self.nodes, &self.funding_keys);
        self.nodes[0] < self.nodes[1]
            && self.nodes.iter().zip(&self.signatures).all(|(node, signature)| verify(&node.0, &digest, &signature.0))
    }
}

/// BOLT 7, channel_update: what one end asks to pass payments on.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelTerms {
    pub funding: OutPoint,
    pub from: NodeId,
    pub base_fee: u64,
    /// Parts per million of the amount passed on.
    pub rate: u32,
    /// Blocks it keeps between the expiry it is offered and the one it offers on.
    pub delta: u16,
    pub stamp: u64,
    pub open: bool,
    pub signature: Signature,
}

impl ChannelTerms {
    fn digest(&self) -> Hash {
        let mut data = b"olc terms".to_vec();
        data.extend_from_slice(&self.funding.txid);
        data.extend_from_slice(&self.funding.index.to_le_bytes());
        data.extend_from_slice(&self.from.0);
        data.extend_from_slice(&self.base_fee.to_le_bytes());
        data.extend_from_slice(&self.rate.to_le_bytes());
        data.extend_from_slice(&self.delta.to_le_bytes());
        data.extend_from_slice(&self.stamp.to_le_bytes());
        data.push(u8::from(self.open));
        hash(&data)
    }

    pub fn sign(&mut self, secret: &[u8; 32]) {
        self.signature = Bytes(sign(secret, &self.digest()).unwrap_or([0; 64]));
    }

    pub fn signed(&self) -> bool {
        verify(&self.from.0, &self.digest(), &self.signature.0)
    }

    /// What it takes to pass `amount` on.
    pub fn fee_for(&self, amount: u64) -> u64 {
        let rated = u128::from(amount) * u128::from(self.rate) / 1_000_000;
        self.base_fee.saturating_add(u64::try_from(rated).unwrap_or(u64::MAX))
    }
}

/// Every channel message, signed by the node that sent it. `to` is all zeros for gossip.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Envelope {
    pub from: NodeId,
    pub to: NodeId,
    pub body: Vec<u8>,
    pub signature: Signature,
}

pub const EVERYONE: NodeId = Bytes([0; 33]);

fn envelope_digest(to: &NodeId, body: &[u8]) -> Hash {
    let mut data = b"olc wire".to_vec();
    data.extend_from_slice(&to.0);
    data.extend_from_slice(body);
    hash(&data)
}

impl Envelope {
    pub fn seal(secret: &[u8; 32], from: NodeId, to: NodeId, wire: &Wire) -> Vec<u8> {
        let body = postcard::to_allocvec(wire).unwrap_or_default();
        let signature = Bytes(sign(secret, &envelope_digest(&to, &body)).unwrap_or([0; 64]));
        postcard::to_allocvec(&Self { from, to, body, signature }).unwrap_or_default()
    }

    /// The sender and what it said, when the signature holds.
    pub fn open(bytes: &[u8]) -> Option<(NodeId, NodeId, Wire)> {
        let envelope: Self = postcard::from_bytes(bytes).ok()?;
        let digest = envelope_digest(&envelope.to, &envelope.body);
        if !verify(&envelope.from.0, &digest, &envelope.signature.0) {
            return None;
        }
        let wire = postcard::from_bytes(&envelope.body).ok()?;
        Some((envelope.from, envelope.to, wire))
    }
}

/// A signature as the wire carries it.
pub fn wrap(signature: SignatureBytes) -> Signature {
    Bytes(signature)
}
