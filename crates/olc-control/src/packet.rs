//! What travels between boards and the computer that runs them.
use crate::beacon::SignedBeacon;
use alloc::string::String;
use alloc::vec::Vec;
use olc_crypto::Hash;
use serde::{Deserialize, Serialize};

/// A board's Wi-Fi station MAC address. A computer picks its own.
pub type BoardId = [u8; 6];

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Packet {
    pub from: BoardId,
    /// `None` is everyone.
    pub to: Option<BoardId>,
    /// With `from`, tells a packet from its copies.
    pub id: u32,
    pub hops: u8,
    pub body: Body,
}

impl Packet {
    pub fn encode(&self) -> Vec<u8> {
        postcard::to_allocvec(self).unwrap_or_default()
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        postcard::from_bytes(bytes).ok()
    }
}

// A packet lives for one trip, so its size here costs nothing that a box would save.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
/// A packet lives for one trip, so a large beacon costs nothing that boxing it would save.
#[allow(clippy::large_enum_variant)]
pub enum Body {
    /// A board saying what it shows on its own, every few seconds.
    Beacon(SignedBeacon),
    /// A question anyone may ask. The answer comes back as `Line`.
    Read(Read),
    /// A line the board printed, for whoever is running it.
    Line(String),
    Order(Signed),
    Chunk {
        session: u32,
        offset: u32,
        data: Vec<u8>,
    },
    /// The update needs the bytes from `next` on.
    Progress {
        session: u32,
        next: u32,
    },
    Refused(Refusal),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Read {
    Status,
    Tx(Hash),
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Beacon {
    pub name: String,
    pub address: [u8; 20],
    pub height: u64,
    pub tip: Hash,
    pub balance: u64,
    pub hashrate: u32,
    pub limit: u32,
    pub peers: u16,
    pub pool: u16,
    pub mining: bool,
    pub offline: bool,
    pub view: String,
    pub language: String,
    pub build: String,
    /// The app slot the board runs from.
    pub slot: u8,
    /// The last counter it accepted. The next order needs a higher one.
    pub counter: u64,
    pub trusts_a_key: bool,
    /// Orders and images it has refused since it started.
    pub refused: u32,
    /// The coin's frames per second on the panel, and how far the setting goes.
    pub smooth: u32,
    pub smooth_range: (u32, u32),
    /// The coin it runs and the work behind its tip, to compare with what it says when asked.
    pub genesis: Hash,
    pub work: u128,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Order {
    pub board: BoardId,
    pub counter: u64,
    pub action: Action,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Action {
    /// A line typed as if on the board's own cable.
    Line(String),
    /// A new image of `size` bytes follows as chunks.
    Update {
        session: u32,
        size: u32,
        image: Hash,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Signed {
    pub order: Order,
    pub signature: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Refusal {
    /// The board has not been given a control key.
    NoKey,
    BadSignature,
    WrongBoard,
    /// The counter was used before.
    Replayed,
    /// Only the board's own cable may do this.
    CableOnly,
    /// The image does not fit the update slot.
    TooLarge,
    /// The whole image does not match the signed hash. The running build stays.
    BadImage,
    /// Flash failed. The running build stays.
    FlashFailed,
    NoUpdateRunning,
}
