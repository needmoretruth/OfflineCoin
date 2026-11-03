//! How a node says what it is. Any device can ask any node, whichever coin either runs, so an app
//! can list what is around and join a coin with no file: the coin travels in the answer, and its
//! genesis id is computed from it, so a node cannot pass one coin off as another.
use alloc::string::String;
use alloc::vec::Vec;
use olc_core::coin::SpokenAdded;
use olc_core::{Address, CoinConfig, Hash};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

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
    /// How the node that passed this on reaches this one, such as `bluetooth`.
    pub via: Via,
    /// Goes last in a hello, after what `Card` writes. See [`Spoken`].
    #[serde(skip)]
    pub keeps: Keeps,
}

/// Comes last, so older builds stop reading before it and answer without it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Via(pub String);

impl<'de> Deserialize<'de> for Via {
    fn deserialize<D: Deserializer<'de>>(reader: D) -> Result<Self, D::Error> {
        Ok(Self(String::deserialize(reader).unwrap_or_default()))
    }
}

impl From<&str> for Via {
    fn from(text: &str) -> Self {
        Self(text.into())
    }
}

/// What a node holds of its chain. It goes as one number after everything older builds read, so
/// they stop before it, and a number from a later build reads as unsaid.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Keeps {
    /// A build too old to say.
    #[default]
    Unsaid,
    Everything,
    Recent,
    Headers,
}

impl Serialize for Keeps {
    fn serialize<S: Serializer>(&self, writer: S) -> Result<S::Ok, S::Error> {
        writer.serialize_u8(match self {
            Self::Unsaid => 0,
            Self::Everything => 1,
            Self::Recent => 2,
            Self::Headers => 3,
        })
    }
}

impl<'de> Deserialize<'de> for Keeps {
    fn deserialize<D: Deserializer<'de>>(reader: D) -> Result<Self, D::Error> {
        Ok(match u8::deserialize(reader).unwrap_or_default() {
            1 => Self::Everything,
            2 => Self::Recent,
            3 => Self::Headers,
            _ => Self::Unsaid,
        })
    }
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
    added: SpokenAdded,
    keeps: Keeps,
    upgrade: Upgrade,
}

/// The coin's upgrade height, after everything else, so older builds stop before it. A hello from
/// one of them leaves it unsaid, and the coin then settles as a file from before it does.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
struct Upgrade(Option<u64>);

impl<'de> Deserialize<'de> for Upgrade {
    fn deserialize<D: Deserializer<'de>>(reader: D) -> Result<Self, D::Error> {
        Ok(Self(Option::<u64>::deserialize(reader).unwrap_or_default()))
    }
}

impl From<Spoken> for Hello {
    fn from(spoken: Spoken) -> Self {
        let Spoken { coin, tip, height, work, address, mut card, added, keeps, upgrade } = spoken;
        card.keeps = keeps;
        let mut coin = coin.with_added(added.0);
        if let Some(from) = upgrade.0 {
            coin.upgrade_from = from;
        }
        Self { coin, tip, height, work, address, card }
    }
}

impl From<Hello> for Spoken {
    fn from(hello: Hello) -> Self {
        let Hello { coin, tip, height, work, address, card } = hello;
        let added = SpokenAdded(coin.added());
        let keeps = card.keeps;
        let upgrade = Upgrade(coin.upgrade_said());
        Self { coin, tip, height, work, address, card, added, keeps, upgrade }
    }
}

impl Hello {
    pub fn genesis(&self) -> Hash {
        self.coin.genesis().id()
    }
}
