//! How a transaction is written. A version 1 transaction is written exactly as before the
//! upgrade, so every block already on a chain keeps its bytes and its id. From
//! [`UNLOCK_FREE_VERSION`] each input also carries its sequence and the transaction its lock time,
//! as Bitcoin's do. A build from before the upgrade cannot read that form, which is the break the
//! upgrade height announces.
use crate::types::{OutPoint, Tx, TxIn, TxOut, SEQUENCE_FINAL, UNLOCK_FREE_VERSION};
use alloc::vec::Vec;
use core::fmt;
use serde::de::{DeserializeSeed, Error as _, SeqAccess, Visitor};
use serde::ser::{SerializeSeq, SerializeTuple};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

fn full(version: u32) -> bool {
    version >= UNLOCK_FREE_VERSION
}

/// The text form, as JSON had it before, with the two new fields only when the version has them.
#[derive(Serialize, Deserialize)]
struct TxText {
    version: u32,
    inputs: Vec<TxInText>,
    outputs: Vec<TxOut>,
    coinbase_height: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    lock_time: Option<u32>,
}

#[derive(Serialize, Deserialize)]
struct TxInText {
    previous: OutPoint,
    unlock: Vec<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sequence: Option<u32>,
}

impl From<&Tx> for TxText {
    fn from(tx: &Tx) -> Self {
        let full = full(tx.version);
        let input = |input: &TxIn| TxInText {
            previous: input.previous,
            unlock: input.unlock.clone(),
            sequence: full.then_some(input.sequence),
        };
        Self {
            version: tx.version,
            inputs: tx.inputs.iter().map(input).collect(),
            outputs: tx.outputs.clone(),
            coinbase_height: tx.coinbase_height,
            lock_time: full.then_some(tx.lock_time),
        }
    }
}

impl From<TxText> for Tx {
    fn from(text: TxText) -> Self {
        let input = |input: TxInText| TxIn {
            previous: input.previous,
            unlock: input.unlock,
            sequence: input.sequence.unwrap_or(SEQUENCE_FINAL),
        };
        Self {
            version: text.version,
            inputs: text.inputs.into_iter().map(input).collect(),
            outputs: text.outputs,
            coinbase_height: text.coinbase_height,
            lock_time: text.lock_time.unwrap_or(0),
        }
    }
}

struct Inputs<'a>(&'a [TxIn], bool);
struct Input<'a>(&'a TxIn, bool);

impl Serialize for Inputs<'_> {
    fn serialize<S: Serializer>(&self, writer: S) -> Result<S::Ok, S::Error> {
        let mut seq = writer.serialize_seq(Some(self.0.len()))?;
        for input in self.0 {
            seq.serialize_element(&Input(input, self.1))?;
        }
        seq.end()
    }
}

impl Serialize for Input<'_> {
    fn serialize<S: Serializer>(&self, writer: S) -> Result<S::Ok, S::Error> {
        let mut tuple = writer.serialize_tuple(if self.1 { 3 } else { 2 })?;
        tuple.serialize_element(&self.0.previous)?;
        tuple.serialize_element(&self.0.unlock)?;
        if self.1 {
            tuple.serialize_element(&self.0.sequence)?;
        }
        tuple.end()
    }
}

impl Serialize for Tx {
    fn serialize<S: Serializer>(&self, writer: S) -> Result<S::Ok, S::Error> {
        if writer.is_human_readable() {
            return TxText::from(self).serialize(writer);
        }
        let full = full(self.version);
        let mut tuple = writer.serialize_tuple(if full { 5 } else { 4 })?;
        tuple.serialize_element(&self.version)?;
        tuple.serialize_element(&Inputs(&self.inputs, full))?;
        tuple.serialize_element(&self.outputs)?;
        tuple.serialize_element(&self.coinbase_height)?;
        if full {
            tuple.serialize_element(&self.lock_time)?;
        }
        tuple.end()
    }
}

struct TxReader;
struct InputsReader(bool);
struct InputReader(bool);

impl<'de> Visitor<'de> for TxReader {
    type Value = Tx;

    fn expecting(&self, out: &mut fmt::Formatter) -> fmt::Result {
        out.write_str("a transaction")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Tx, A::Error> {
        let missing = || A::Error::custom("a transaction ends early");
        let version: u32 = seq.next_element()?.ok_or_else(missing)?;
        let full = full(version);
        let inputs = seq.next_element_seed(InputsReader(full))?.ok_or_else(missing)?;
        let outputs = seq.next_element()?.ok_or_else(missing)?;
        let coinbase_height = seq.next_element()?.ok_or_else(missing)?;
        let lock_time = match full {
            true => seq.next_element()?.ok_or_else(missing)?,
            false => 0,
        };
        Ok(Tx { version, inputs, outputs, coinbase_height, lock_time })
    }
}

impl<'de> DeserializeSeed<'de> for InputsReader {
    type Value = Vec<TxIn>;

    fn deserialize<D: Deserializer<'de>>(self, reader: D) -> Result<Self::Value, D::Error> {
        reader.deserialize_seq(self)
    }
}

impl<'de> Visitor<'de> for InputsReader {
    type Value = Vec<TxIn>;

    fn expecting(&self, out: &mut fmt::Formatter) -> fmt::Result {
        out.write_str("inputs")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
        let mut inputs = Vec::new();
        while let Some(input) = seq.next_element_seed(InputReader(self.0))? {
            inputs.push(input);
        }
        Ok(inputs)
    }
}

impl<'de> DeserializeSeed<'de> for InputReader {
    type Value = TxIn;

    fn deserialize<D: Deserializer<'de>>(self, reader: D) -> Result<Self::Value, D::Error> {
        reader.deserialize_tuple(if self.0 { 3 } else { 2 }, self)
    }
}

impl<'de> Visitor<'de> for InputReader {
    type Value = TxIn;

    fn expecting(&self, out: &mut fmt::Formatter) -> fmt::Result {
        out.write_str("an input")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<TxIn, A::Error> {
        let missing = || A::Error::custom("an input ends early");
        let previous = seq.next_element()?.ok_or_else(missing)?;
        let unlock = seq.next_element()?.ok_or_else(missing)?;
        let sequence = match self.0 {
            true => seq.next_element()?.ok_or_else(missing)?,
            false => SEQUENCE_FINAL,
        };
        Ok(TxIn { previous, unlock, sequence })
    }
}

impl<'de> Deserialize<'de> for Tx {
    fn deserialize<D: Deserializer<'de>>(reader: D) -> Result<Self, D::Error> {
        if reader.is_human_readable() {
            return TxText::deserialize(reader).map(Tx::from);
        }
        reader.deserialize_tuple(5, TxReader)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shape every build before the upgrade wrote, with serde's own derive.
    #[derive(Serialize)]
    struct OldTx {
        version: u32,
        inputs: Vec<OldIn>,
        outputs: Vec<TxOut>,
        coinbase_height: Option<u64>,
    }

    #[derive(Serialize)]
    struct OldIn {
        previous: OutPoint,
        unlock: Vec<u8>,
    }

    fn tx(version: u32) -> Tx {
        let previous = OutPoint { txid: [7; 32], index: 3 };
        Tx {
            version,
            inputs: alloc::vec![TxIn { previous, unlock: alloc::vec![1, 2, 3], sequence: 9 }],
            outputs: alloc::vec![TxOut { value: 5, language: 0, lock: alloc::vec![4; 20] }],
            coinbase_height: None,
            lock_time: 77,
        }
    }

    #[test]
    fn a_first_version_transaction_keeps_its_old_bytes() {
        let new = tx(1);
        let old = OldTx {
            version: 1,
            inputs: alloc::vec![OldIn { previous: new.inputs[0].previous, unlock: alloc::vec![1, 2, 3] }],
            outputs: new.outputs.clone(),
            coinbase_height: None,
        };
        let bytes = postcard::to_allocvec(&new).unwrap();
        assert_eq!(bytes, postcard::to_allocvec(&old).unwrap());
        let read: Tx = postcard::from_bytes(&bytes).unwrap();
        assert_eq!((read.lock_time, read.inputs[0].sequence), (0, SEQUENCE_FINAL));
    }

    #[test]
    fn a_second_version_transaction_carries_its_sequence_and_lock_time() {
        let new = tx(2);
        let bytes = postcard::to_allocvec(&new).unwrap();
        assert_eq!(postcard::from_bytes::<Tx>(&bytes).unwrap(), new);
        let text = serde_json::to_string(&new).unwrap();
        assert_eq!(serde_json::from_str::<Tx>(&text).unwrap(), new);
        let old_text = serde_json::to_string(&tx(1)).unwrap();
        assert!(!old_text.contains("sequence") && !old_text.contains("lock_time"));
    }
}
