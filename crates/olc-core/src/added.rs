//! What a coin gained after older builds fixed its binary layout, and how it travels after it.
use crate::coin::{ForkBytes, NEVER};
use crate::timing::{chosen, Timing};
use alloc::vec::Vec;
use core::fmt;
use serde::de::{SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// What a coin gained after older builds fixed its binary layout. It travels after the end of
/// a message, where they stop reading, and a message from one of them just ends before it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CoinAdded {
    pub(crate) min_difficulty: u64,
    pub(crate) message_in_block: bool,
    pub(crate) floor_from_height: u64,
    pub(crate) forks: Vec<ForkBytes>,
    /// Comes after the forks, where older builds stop. A coin from before it leaves it out.
    pub(crate) upgrade_from: Option<u64>,
    /// Difficulty and time of the base rules, then of each fork, and whether blocks carry
    /// Bitcoin's header, after the upgrade height. A coin that never chose leaves them out.
    pub(crate) timings: Vec<Timing>,
    pub(crate) bitcoin_header: bool,
}

impl Serialize for CoinAdded {
    /// A coin without forks or a signing height stays byte for byte what it was.
    fn serialize<S: Serializer>(&self, writer: S) -> Result<S::Ok, S::Error> {
        let first = (self.min_difficulty, self.message_in_block, self.floor_from_height);
        if chosen(&self.timings) || self.bitcoin_header {
            let from = self.upgrade_from.unwrap_or(NEVER);
            let later = (&self.timings, self.bitcoin_header);
            return (first.0, first.1, first.2, &self.forks, from, later).serialize(writer);
        }
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
            if added.upgrade_from.is_some() && self.fields > 5 {
                let later: Option<(Vec<Timing>, bool)> = fields.next_element().ok().flatten();
                (added.timings, added.bitcoin_header) = later.unwrap_or_default();
            }
            added.forks = forks.unwrap_or_default();
        }
        Ok(added)
    }
}

impl<'de> Deserialize<'de> for CoinAdded {
    fn deserialize<D: Deserializer<'de>>(reader: D) -> Result<Self, D::Error> {
        Ok(reader.deserialize_tuple(6, AddedFields { fields: 6 }).unwrap_or_default())
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
    /// Stops after the forks, where the message goes on. A hello carries the signing height and
    /// the coin's difficulty and time rules at its own end.
    fn deserialize<D: Deserializer<'de>>(reader: D) -> Result<Self, D::Error> {
        Ok(Self(reader.deserialize_tuple(4, AddedFields { fields: 4 }).unwrap_or_default()))
    }
}

