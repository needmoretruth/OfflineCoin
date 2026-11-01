//! How a node says what it is. Any device can ask any node, whichever coin either runs, so an app
//! can list what is around and join a coin with no file: the coin travels in the answer, and its
//! genesis id is computed from it, so a node cannot pass one coin off as another.
use alloc::string::String;
use alloc::vec::Vec;
use olc_core::coin::CoinAdded;
use olc_core::{Address, CoinConfig, Hash};
use serde::{Deserialize, Serialize};

/// Addresses to pass on in one answer, as in Bitcoin's addr message.
pub const MAX_KNOWN: usize = 16;

/// What a device says about itself. All of it is the device's own word.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Card {
    pub name: String,
    /// What it runs on, such as `computer`, `raspberry-pi` or `esp32-s3`.
    pub device: String,
    pub build: String,
    /// The TCP port it listens on, or 0.
    pub port: u16,
    pub mining: bool,
    pub hashrate: u64,
    /// Other nodes it knows, as `host:port`.
    pub known: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "Spoken", into = "Spoken")]
pub struct Hello {
    pub coin: CoinConfig,
    pub tip: Hash,
    pub height: u64,
    pub work: u128,
    pub address: Address,
    pub card: Card,
}

/// A hello as it goes over the wire: the fields older builds read, then what they skip.
#[derive(Clone, Serialize, Deserialize)]
struct Spoken {
    coin: CoinConfig,
    tip: Hash,
    height: u64,
    work: u128,
    address: Address,
    card: Card,
    added: CoinAdded,
}

impl From<Spoken> for Hello {
    fn from(spoken: Spoken) -> Self {
        let Spoken { coin, tip, height, work, address, card, added } = spoken;
        Self { coin: coin.with_added(added), tip, height, work, address, card }
    }
}

impl From<Hello> for Spoken {
    fn from(hello: Hello) -> Self {
        let Hello { coin, tip, height, work, address, card } = hello;
        let added = coin.added();
        Self { coin, tip, height, work, address, card, added }
    }
}

impl Hello {
    pub fn genesis(&self) -> Hash {
        self.coin.genesis().id()
    }
}
