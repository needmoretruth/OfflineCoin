//! A coin is a genesis block plus rules.
//!
//! The genesis decides which chain a node is on. The rules decide which blocks a node accepts.
//! As in Bitcoin they are separate: edit the rules on some nodes and those nodes fork away from
//! the others on the same chain history, and no new coin is needed.
use crate::types::{merkle_root, Block, BlockHeader, Tx, TxOut};
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use olc_crypto::{Address, Hash};
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

/// The height OLC takes version 2 transactions from, ahead of the chain when it was released. A
/// version 2 transaction carries what Bitcoin added after Satoshi:
/// - its signatures name the coin they are for, so it cannot be replayed on a fork of OLC or on
///   another coin, nor one of theirs here (see [`CoinConfig::signing_id_at`]), and each may carry
///   one of Bitcoin's signature kinds (see [`crate::sighash`]);
/// - it is named without its unlock data, as SegWit's txid is (see [`crate::Tx::id`]);
/// - it carries Bitcoin's lock time and sequences (BIP 68), and spends gamma locks, today's
///   Bitcoin Script.
///
/// Version 1 transactions stay valid beside them in the same blocks, as legacy transactions do
/// beside SegWit ones in Bitcoin: same bytes, same name, same signatures as ever. A fork made
/// with new rules takes only version 2 from its height (see
/// [`CoinConfig::takes_version_one_at`]). As with the floor, every node runs a build that knows
/// this before the chain gets there: a block with a version 2 transaction splits from nodes that
/// do not.
pub const OLC_UPGRADE_FROM: u64 = 6_240;

/// A coin made before version 2, whose history stays valid as it is. It takes version 2 from its
/// first fork, if it ever forks.
pub const NEVER: u64 = u64::MAX;

/// A file or message from before the upgrade said nothing. [`CoinConfig::settle_floor`] makes it
/// [`OLC_UPGRADE_FROM`] on OLC's genesis and [`NEVER`] on any other.
pub const UNSAID: u64 = u64::MAX - 1;

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
    /// The height version 2 transactions start at, as [`OLC_UPGRADE_FROM`] lists them: 0 for a
    /// coin made since, [`NEVER`] for a coin made before them, which takes them from its first
    /// fork.
    #[serde(default = "unsaid", serialize_with = "text_only", deserialize_with = "read_text_or_unsaid")]
    pub upgrade_from: u64,
}

fn unsaid() -> u64 {
    UNSAID
}

/// As [`read_text_only`], where a binary form without the field means a coin from before it.
fn read_text_or_unsaid<'de, D: Deserializer<'de>>(reader: D) -> Result<u64, D::Error> {
    match reader.is_human_readable() {
        true => u64::deserialize(reader),
        false => Ok(UNSAID),
    }
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
    /// Comes after the forks, where older builds stop. A coin from before it leaves it out.
    upgrade_from: Option<u64>,
}

impl Serialize for CoinAdded {
    /// A coin without forks or a signing height stays byte for byte what it was.
    fn serialize<S: Serializer>(&self, writer: S) -> Result<S::Ok, S::Error> {
        let first = (self.min_difficulty, self.message_in_block, self.floor_from_height);
        match (self.forks.is_empty(), self.upgrade_from) {
            (_, Some(from)) => (first.0, first.1, first.2, &self.forks, from).serialize(writer),
            (true, None) => first.serialize(writer),
            (false, None) => (first.0, first.1, first.2, &self.forks).serialize(writer),
        }
    }
}

/// Reads what a coin gained. `fields` is 4 where a message goes on after the forks.
struct AddedFields {
    fields: usize,
}

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
            let forks: Option<Vec<ForkBytes>> = fields.next_element().ok().flatten();
            if forks.is_some() && self.fields > 4 {
                added.upgrade_from = fields.next_element().ok().flatten();
            }
            added.forks = forks.unwrap_or_default();
        }
        Ok(added)
    }
}

impl<'de> Deserialize<'de> for CoinAdded {
    fn deserialize<D: Deserializer<'de>>(reader: D) -> Result<Self, D::Error> {
        Ok(reader.deserialize_tuple(5, AddedFields { fields: 5 }).unwrap_or_default())
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
    /// Stops after the forks, where the message goes on. A hello carries the signing height at
    /// its own end.
    fn deserialize<D: Deserializer<'de>>(reader: D) -> Result<Self, D::Error> {
        Ok(Self(reader.deserialize_tuple(4, AddedFields { fields: 4 }).unwrap_or_default()))
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
            upgrade_from: OLC_UPGRADE_FROM,
        }
    }

    /// What a signature in a block at `height` names, from [`CoinConfig::upgrade_from`] on: the
    /// genesis and every rule in force there, the forks whose height has come included. A fork
    /// and the coin it left name different things from the fork's height, and two coins always
    /// do, so neither takes a payment signed for the other. `None` below that height, where a
    /// signature names no coin, as it always did.
    pub fn signing_id_at(&self, height: u64) -> Option<Hash> {
        if !self.upgraded_at(height) {
            return None;
        }
        let come = self.forks.iter().filter(|fork| fork.from_height <= height);
        let forks: Vec<ForkBytes> = come.map(Fork::bytes).collect();
        let rules = (self.rules.min_difficulty, self.rules.floor_from_height, self.upgrade_from);
        let named = (b"OLC coin", self.genesis().id(), &self.rules, rules, forks);
        Some(olc_crypto::hash(&postcard::to_allocvec(&named).unwrap_or_default()))
    }

    /// True when a block at `height` takes version 2 transactions: from the upgrade, or from the
    /// first fork, whichever comes first.
    pub fn upgraded_at(&self, height: u64) -> bool {
        height >= self.upgrade_from || !self.takes_version_one_at(height)
    }

    /// True when a block at `height` takes version 1 transactions: always on a coin that never
    /// forked, as Bitcoin still takes legacy ones, and below the first fork's height on one that
    /// did. From there every payment names its coin, so neither side of a fork can replay the
    /// other's.
    pub fn takes_version_one_at(&self, height: u64) -> bool {
        !self.forks.iter().any(|fork| fork.from_height <= height)
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
        let base = |coin: &Self| (coin.rules.clone(), coin.rules.min_difficulty, coin.rules.floor_from_height);
        if base(self) != base(other) {
            return Some(0);
        }
        let same = self.forks.iter().zip(&other.forks).take_while(|(ours, theirs)| ours == theirs);
        let same = same.count();
        let next = |coin: &Self| coin.forks.get(same).map(|fork| fork.from_height);
        let signing = (self.upgrade_from != other.upgrade_from)
            .then(|| self.upgrade_from.min(other.upgrade_from));
        [next(self), next(other), signing].into_iter().flatten().min().or(Some(0))
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
    /// A coin from before signatures named their coin keeps the name it had.
    pub fn rules_id(&self) -> String {
        let forks: Vec<ForkBytes> = self.forks.iter().map(Fork::bytes).collect();
        let rules = (&self.rules, self.rules.min_difficulty, self.rules.floor_from_height, forks);
        let mut bytes = postcard::to_allocvec(&rules).unwrap_or_default();
        if self.upgrade_from != NEVER {
            bytes.extend(postcard::to_allocvec(&self.upgrade_from).unwrap_or_default());
        }
        short_name(&bytes)
    }

    /// Fills in a floor left open by a file or answer from before floors. OLC takes its own, and
    /// any other coin was made when the only limit was 1. The same goes for the height signatures
    /// name the coin from: OLC and every fork of it take OLC's, and any other coin left without
    /// one was made before it, so its signatures never name it.
    pub fn settle_floor(&mut self) {
        if self.upgrade_from == UNSAID {
            self.upgrade_from = match self.is_olc() {
                true => OLC_UPGRADE_FROM,
                false => NEVER,
            };
        }
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
            upgrade_from: self.upgrade_said(),
        }
    }

    /// Does this coin start from OLC's genesis, as OLC and its forks do?
    pub fn is_olc(&self) -> bool {
        self.genesis().id() == Self::olc().genesis().id()
    }

    /// A coin from before the upgrade leaves the height out, so its bytes stay as they were.
    pub fn upgrade_said(&self) -> Option<u64> {
        (self.upgrade_from != NEVER).then_some(self.upgrade_from)
    }

    pub fn with_added(mut self, added: CoinAdded) -> Self {
        self.rules.min_difficulty = added.min_difficulty;
        self.rules.floor_from_height = added.floor_from_height;
        self.message_in_block = added.message_in_block;
        self.forks = added.forks.into_iter().map(Fork::from_bytes).collect();
        self.upgrade_from = added.upgrade_from.unwrap_or(UNSAID);
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
            lock_time: 0,
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
