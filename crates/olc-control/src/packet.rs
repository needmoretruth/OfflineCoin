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

/// The start of a beacon from a build older than this format. Later fields moved or did not
/// exist yet, so nothing past `build` can be read and no signature can be checked: it shows that
/// a board is there and what it says, never who it is for certain.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OldBeacon {
    pub from: BoardId,
    pub to: Option<BoardId>,
    pub id: u32,
    pub hops: u8,
    body_kind: u32,
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
}

impl OldBeacon {
    /// Reads bytes that `Packet::decode` could not. Anything but a beacon is `None`.
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        postcard::from_bytes(bytes)
            .ok()
            .filter(|old: &Self| old.body_kind == BEACON_KIND)
    }

    pub fn encode(&self) -> Vec<u8> {
        postcard::to_allocvec(self).unwrap_or_default()
    }

    /// A stand-in with the same route, for the router to tell copies apart.
    pub fn route(&self) -> Packet {
        Packet {
            from: self.from,
            to: self.to,
            id: self.id,
            hops: self.hops,
            body: Body::Beacon(SignedBeacon::default()),
        }
    }

    pub fn beacon(self) -> Beacon {
        Beacon {
            name: self.name,
            address: self.address,
            height: self.height,
            tip: self.tip,
            balance: self.balance,
            hashrate: self.hashrate,
            limit: self.limit,
            peers: self.peers,
            pool: self.pool,
            mining: self.mining,
            offline: self.offline,
            view: self.view,
            language: self.language,
            build: self.build,
            ..Beacon::default()
        }
    }
}

/// Where `Body::Beacon` stands in `Body`.
const BEACON_KIND: u32 = 0;

// A packet lives for one trip, so its size here costs nothing that a box would save.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
    /// One piece of an image. Nothing answers it: the hash decides in the end.
    Piece {
        session: u32,
        index: u32,
        data: Vec<u8>,
    },
    AskMissing {
        session: u32,
    },
    /// `done` counts bytes arrived plus bytes written. `runs` are the first (piece, count) still
    /// missing. Empty means everything arrived.
    Missing {
        session: u32,
        done: u32,
        runs: Vec<(u32, u32)>,
    },
    /// Pool work for a board this computer reaches only through another. See [`crate::lend`].
    Lend(crate::lend::Lent),
    /// Headers that reached lent work's difficulty, from the board that ground them. Sent with
    /// none the moment it takes signed work, so the computer knows it was taken.
    Shares {
        worker: [u8; 20],
        headers: Vec<Vec<u8>>,
    },
    /// From the board on the cable: it passed signed work on to the board that mines to `worker`.
    Passed { worker: [u8; 20] },
    /// The computer's time, for boards that have no clock of the date. See [`crate::dated`].
    /// Builds before i21 cannot read it and pass it by.
    Dated(crate::dated::Dated),
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
    /// A new image of `size` bytes follows as pieces. Builds from before pieces cannot read
    /// this order and stay silent, so the sender falls back on `Update`.
    Burst {
        session: u32,
        size: u32,
        image: Hash,
    },
    /// Send the build this board runs to `target`, if it is the one with this hash.
    Serve {
        target: BoardId,
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

impl core::fmt::Display for Refusal {
    fn fmt(&self, out: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        out.write_str(match self {
            Self::NoKey => "the board has no control key yet",
            Self::BadSignature => "the order was not signed with this computer's control key",
            Self::WrongBoard => "the order was meant for another board",
            Self::Replayed => "the order came twice",
            Self::CableOnly => "only the board's own cable may do this",
            Self::TooLarge => "the build does not fit the board's update space",
            Self::BadImage => "the build arrived damaged, so the board kept the one it runs",
            Self::FlashFailed => "the board could not write the build, so it kept the one it runs",
            Self::NoUpdateRunning => "no update was under way",
        })
    }
}
