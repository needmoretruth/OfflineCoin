//! A coin is a genesis block plus rules.
//!
//! The genesis decides which chain a node is on. The rules decide which blocks a node accepts.
//! As in Bitcoin they are separate: edit the rules on some nodes and those nodes fork away from
//! the others on the same chain history, and no new coin is needed.
use crate::types::{merkle_root, Block, BlockHeader, Tx, TxOut};
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use olc_crypto::Address;
use core::fmt;
use serde::de::{SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// One coin is 100,000,000 base units, as in Bitcoin.
pub const COIN: u64 = 100_000_000;

/// How fast the one device a coin must be able to live on hashes.
pub const LOWEST_HASHES_A_SECOND: u64 = 1_000;

/// The floor and first difficulty of a new coin: that device finds a block every 30 seconds.
pub const MIN_DIFFICULTY: u64 = 30 * LOWEST_HASHES_A_SECOND;

/// OLC blocks 1,180 to 1,209 sit below the floor and stay valid, and difficulty still comes close
/// to it, so the floor starts at a height ahead of the chain. Every node has to run a build with
/// the floor before then, as with a soft fork. This must stay above the tip until it is released.
const OLC_FLOOR_FROM_HEIGHT: u64 = 20_000;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rules {
    /// Target time between blocks.
    pub block_time_ms: u64,
    /// Blocks between difficulty changes. 0 keeps the difficulty fixed.
    pub retarget_interval: u64,
    /// Reward of the first block, in base units.
    pub reward: u64,
    /// Blocks between reward halvings. 0 never halves.
    pub halving_interval: u64,
    /// Blocks before a coinbase output can be spent.
    pub coinbase_maturity: u64,
    pub max_block_bytes: u64,
    /// Lowest fee a transaction may pay, in base units.
    pub min_fee: u64,
    /// The retarget never goes below this, as with Bitcoin's powLimit. 0 means a file or answer
    /// from before floors: see [`CoinConfig::settle_floor`]. Binary forms leave it out, see
    /// [`CoinAdded`].
    #[serde(default, serialize_with = "text_only", deserialize_with = "read_text_only")]
    pub min_difficulty: u64,
    /// The height the floor starts at, 0 for a coin that always had one. As with Bitcoin's buried
    /// deployments, this lets a floor join a chain whose older blocks went below it.
    #[serde(default, serialize_with = "text_only", deserialize_with = "read_text_only")]
    pub floor_from_height: u64,
}

/// Older builds read a coin in a fixed binary layout, and postcard cannot tell a missing field
/// from the next one, so what came later stays out of that form.
fn text_only<T: Serialize, S: Serializer>(value: &T, serializer: S) -> Result<S::Ok, S::Error> {
    match serializer.is_human_readable() {
        true => value.serialize(serializer),
        false => serializer.serialize_unit(),
    }
}

fn read_text_only<'de, T, D>(reader: D) -> Result<T, D::Error>
where
    T: Default + Deserialize<'de>,
    D: Deserializer<'de>,
{
    match reader.is_human_readable() {
        true => T::deserialize(reader),
        false => Ok(T::default()),
    }
}

impl Default for Rules {
    fn default() -> Self {
        Self {
            block_time_ms: 30_000,
            retarget_interval: 10,
            reward: 50 * COIN,
            halving_interval: 210_000,
            coinbase_maturity: 3,
            max_block_bytes: 4_096,
            min_fee: 0,
            min_difficulty: MIN_DIFFICULTY,
            floor_from_height: 0,
        }
    }
}

impl Rules {
    /// The first height a mining reward from `height` may be spent at.
    pub fn matures_at(&self, height: u64) -> u64 {
        height.saturating_add(self.coinbase_maturity)
    }

    /// The lowest difficulty a block at `height` may carry after a retarget.
    pub fn floor_at(&self, height: u64) -> u64 {
        match height >= self.floor_from_height {
            true => self.min_difficulty,
            false => 1,
        }
    }

    /// The floor at which the lowest device still keeps the block time.
    pub fn floor_for(block_time_ms: u64) -> u64 {
        block_time_ms.saturating_mul(LOWEST_HASHES_A_SECOND) / 1_000
    }

    pub fn reward_at(&self, height: u64) -> u64 {
        let halvings = match self.halving_interval {
            0 => 0,
            interval => height / interval,
        };
        if halvings >= 64 {
            0
        } else {
            self.reward >> halvings
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoinConfig {
    pub name: String,
    pub ticker: String,
    /// Written into the genesis block. Change it to start a separate chain.
    pub message: String,
    pub genesis_time_ms: u64,
    pub initial_difficulty: u64,
    /// Coins that exist from the start, as (address, base units).
    pub premine: Vec<(Address, u64)>,
    pub rules: Rules,
    /// Also keeps the message as data in the genesis block, where anyone can read it. The default
    /// coin's genesis came first, so it only hashes the message.
    #[serde(default, serialize_with = "text_only", deserialize_with = "read_text_only")]
    pub message_in_block: bool,
    /// Rule changes, oldest first. The genesis and history stay, and each applies from its height.
    #[serde(default, serialize_with = "text_only", deserialize_with = "read_text_only")]
    pub forks: Vec<Fork>,
}

/// New rules from a height on, on the same genesis and history.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fork {
    pub from_height: u64,
    pub rules: Rules,
}

/// A fork in binary, where [`Rules`] leaves out what came later: height, rules, floor, floor start.
type ForkBytes = (u64, Rules, u64, u64);

impl Fork {
    fn bytes(&self) -> ForkBytes {
        (self.from_height, self.rules.clone(), self.rules.min_difficulty, self.rules.floor_from_height)
    }

    fn from_bytes(read: ForkBytes) -> Self {
        let (from_height, mut rules, min_difficulty, floor_from_height) = read;
        rules.min_difficulty = min_difficulty;
        rules.floor_from_height = floor_from_height;
        Self { from_height, rules }
    }

    /// Added to the difficulty at the fork's first block. Both sides then refuse each other from
    /// that height whichever rule changed, even a change only one side would notice, and a header
    /// alone shows which side it is on.
    pub fn mark(&self) -> u64 {
        let bytes = postcard::to_allocvec(&self.bytes()).unwrap_or_default();
        1 + u64::from(olc_crypto::hash(&bytes)[0])
    }
}

/// What a coin gained after older builds fixed its binary layout. It travels after the end of
/// a message, where they stop reading, and a message from one of them just ends before it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CoinAdded {
    min_difficulty: u64,
    message_in_block: bool,
    floor_from_height: u64,
    forks: Vec<ForkBytes>,
}

impl Serialize for CoinAdded {
    /// A coin without forks stays byte for byte what it was.
    fn serialize<S: Serializer>(&self, writer: S) -> Result<S::Ok, S::Error> {
        let first = (self.min_difficulty, self.message_in_block, self.floor_from_height);
        match self.forks.is_empty() {
            true => first.serialize(writer),
            false => (first.0, first.1, first.2, &self.forks).serialize(writer),
        }
    }
}

struct AddedFields;

impl<'de> Visitor<'de> for AddedFields {
    type Value = CoinAdded;

    fn expecting(&self, out: &mut fmt::Formatter) -> fmt::Result {
        out.write_str("what a coin gained")
    }

    /// A message from an older build ends after the third field, or before the first.
    fn visit_seq<A: SeqAccess<'de>>(self, mut fields: A) -> Result<CoinAdded, A::Error> {
        let mut added = CoinAdded::default();
        let first = fields.next_element().ok().flatten();
        let second = first.and_then(|_| fields.next_element().ok().flatten());
        let third = second.and_then(|_| fields.next_element().ok().flatten());
        if let (Some(floor), Some(in_block), Some(from)) = (first, second, third) {
            added.min_difficulty = floor;
            added.message_in_block = in_block;
            added.floor_from_height = from;
            added.forks = fields.next_element().ok().flatten().unwrap_or_default();
        }
        Ok(added)
    }
}

impl<'de> Deserialize<'de> for CoinAdded {
    fn deserialize<D: Deserializer<'de>>(reader: D) -> Result<Self, D::Error> {
        Ok(reader.deserialize_tuple(4, AddedFields).unwrap_or_default())
    }
}

/// [`CoinAdded`] with its forks written even when there are none, so a message can go on after it
/// without the next byte reading as forks.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SpokenAdded(pub CoinAdded);

impl Serialize for SpokenAdded {
    fn serialize<S: Serializer>(&self, writer: S) -> Result<S::Ok, S::Error> {
        let added = &self.0;
        (added.min_difficulty, added.message_in_block, added.floor_from_height, &added.forks)
            .serialize(writer)
    }
}

impl<'de> Deserialize<'de> for SpokenAdded {
    fn deserialize<D: Deserializer<'de>>(reader: D) -> Result<Self, D::Error> {
        CoinAdded::deserialize(reader).map(Self)
    }
}

/// The start of `bytes`'s hash, for reading.
fn short_name(bytes: &[u8]) -> String {
    let hash = olc_crypto::hash(bytes);
    let mut text = String::new();
    for byte in &hash[..4] {
        let _ = fmt::Write::write_fmt(&mut text, format_args!("{byte:02x}"));
    }
    text
}

impl CoinConfig {
    /// The default coin. Its first difficulty is in the genesis block and stays. From there the
    /// retarget follows the hash power present, down to one device at 1,000 hashes per second
    /// finding a block every 30 seconds.
    pub fn olc() -> Self {
        Self {
            name: "OfflineCoin".into(),
            ticker: "OLC".into(),
            message: "OLCC genesis".into(),
            genesis_time_ms: 1_793_491_200_000,
            initial_difficulty: 900_000,
            premine: Vec::new(),
            rules: Rules { floor_from_height: OLC_FLOOR_FROM_HEIGHT, ..Rules::default() },
            message_in_block: false,
            forks: Vec::new(),
        }
    }

    /// The rules for a block at `height`.
    pub fn rules_at(&self, height: u64) -> &Rules {
        let fork = self.forks.iter().rev().find(|fork| height >= fork.from_height);
        fork.map_or(&self.rules, |fork| &fork.rules)
    }

    /// The fork whose first block is at `height`.
    pub fn fork_starting_at(&self, height: u64) -> Option<&Fork> {
        self.forks.iter().find(|fork| fork.from_height == height)
    }

    /// Every set of rules this coin has had.
    pub fn all_rules(&self) -> impl Iterator<Item = &Rules> {
        core::iter::once(&self.rules).chain(self.forks.iter().map(|fork| &fork.rules))
    }

    /// The same coin without its last fork, which is the way back from it.
    pub fn before_last_fork(&self) -> Option<Self> {
        let mut coin = self.clone();
        coin.forks.pop()?;
        Some(coin)
    }

    /// A short name for this coin's rule changes. Empty when it has none, so a coin without forks
    /// is named as it always was.
    /// The first height where this coin and `other` go by different rules, if there is one. Blocks
    /// below it mean the same to both, and blocks from it on belong to one side only.
    pub fn parts_from(&self, other: &Self) -> Option<u64> {
        if self.rules_id() == other.rules_id() {
            return None;
        }
        let same = self.forks.iter().zip(&other.forks).take_while(|(ours, theirs)| ours == theirs);
        let same = same.count();
        let next = |coin: &Self| coin.forks.get(same).map(|fork| fork.from_height);
        [next(self), next(other)].into_iter().flatten().min().or(Some(0))
    }

    pub fn forks_id(&self) -> String {
        if self.forks.is_empty() {
            return String::new();
        }
        let forks: Vec<ForkBytes> = self.forks.iter().map(Fork::bytes).collect();
        short_name(&postcard::to_allocvec(&forks).unwrap_or_default())
    }

    /// A short name for every rule this coin runs by, now and at later heights. Two nodes on one
    /// genesis with different names leave each other at the height their rules part. Words a
    /// person sees, such as the coin's name, do not count.
    pub fn rules_id(&self) -> String {
        let forks: Vec<ForkBytes> = self.forks.iter().map(Fork::bytes).collect();
        let rules = (&self.rules, self.rules.min_difficulty, self.rules.floor_from_height, forks);
        short_name(&postcard::to_allocvec(&rules).unwrap_or_default())
    }

    /// Fills in a floor left open by a file or answer from before floors. OLC takes its own, and
    /// any other coin was made when the only limit was 1.
    pub fn settle_floor(&mut self) {
        if self.rules.min_difficulty == 0 {
            let olc = Self::olc();
            self.rules.min_difficulty = 1;
            if self.genesis().id() == olc.genesis().id() {
                self.rules.min_difficulty = olc.rules.min_difficulty;
                self.rules.floor_from_height = olc.rules.floor_from_height;
            }
        }
        self.forks.sort_by_key(|fork| fork.from_height);
        let mut floor = self.rules.min_difficulty;
        for fork in &mut self.forks {
            if fork.rules.min_difficulty == 0 {
                fork.rules.min_difficulty = floor;
            }
            floor = fork.rules.min_difficulty;
        }
    }

    pub fn added(&self) -> CoinAdded {
        CoinAdded {
            min_difficulty: self.rules.min_difficulty,
            message_in_block: self.message_in_block,
            floor_from_height: self.rules.floor_from_height,
            forks: self.forks.iter().map(Fork::bytes).collect(),
        }
    }

    pub fn with_added(mut self, added: CoinAdded) -> Self {
        self.rules.min_difficulty = added.min_difficulty;
        self.rules.floor_from_height = added.floor_from_height;
        self.message_in_block = added.message_in_block;
        self.forks = added.forks.into_iter().map(Fork::from_bytes).collect();
        self.settle_floor();
        self
    }

    /// The coin in the layout older builds read, then what they skip.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = postcard::to_allocvec(self).unwrap_or_default();
        bytes.extend(postcard::to_allocvec(&self.added()).unwrap_or_default());
        bytes
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let (coin, rest) = postcard::take_from_bytes::<Self>(bytes).ok()?;
        let added = postcard::from_bytes(rest).unwrap_or_default();
        Some(coin.with_added(added))
    }

    pub fn genesis(&self) -> Block {
        let mut outputs: Vec<TxOut> = self
            .premine
            .iter()
            .map(|(address, value)| TxOut {
                value: *value,
                language: 0,
                lock: address.to_vec(),
            })
            .collect();
        if self.message_in_block {
            outputs.push(TxOut::data(self.message.as_bytes().to_vec()));
        }
        let tx = Tx {
            version: 1,
            inputs: Vec::new(),
            outputs,
            coinbase_height: Some(0),
        };
        let txs = vec![tx];
        // The name and message are hashed into the root, so they define the chain.
        let mut root_input = merkle_root(&txs).to_vec();
        root_input.extend_from_slice(self.name.as_bytes());
        root_input.extend_from_slice(self.message.as_bytes());
        let header = BlockHeader {
            version: 1,
            previous: [0; 32],
            merkle_root: olc_crypto::hash(&root_input),
            time_ms: self.genesis_time_ms,
            difficulty: self.initial_difficulty,
            nonce: 0,
        };
        Block { header, txs }
    }
}
