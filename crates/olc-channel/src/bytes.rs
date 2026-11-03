//! Fixed byte strings longer than serde's arrays, such as keys and signatures.
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::ops::Deref;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Bytes<const N: usize>(pub [u8; N]);

/// A public key: a node's, or a channel side's.
pub type Key = Bytes<33>;
/// A signature of 64 bytes.
pub type Signature = Bytes<64>;

impl<const N: usize> Deref for Bytes<N> {
    type Target = [u8; N];

    fn deref(&self) -> &[u8; N] {
        &self.0
    }
}

impl<const N: usize> From<[u8; N]> for Bytes<N> {
    fn from(bytes: [u8; N]) -> Self {
        Self(bytes)
    }
}

impl<const N: usize> Serialize for Bytes<N> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_bytes(&self.0)
    }
}

impl<'de, const N: usize> Deserialize<'de> for Bytes<N> {
    fn deserialize<D: Deserializer<'de>>(reader: D) -> Result<Self, D::Error> {
        let bytes: Vec<u8> = Deserialize::deserialize(reader)?;
        let array = bytes.try_into().map_err(|_| serde::de::Error::custom("wrong length"))?;
        Ok(Self(array))
    }
}

/// Lower-case hex, as people copy ids.
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn from_hex(text: &str) -> Option<Vec<u8>> {
    let text = text.trim();
    if !text.len().is_multiple_of(2) || !text.is_ascii() {
        return None;
    }
    (0..text.len()).step_by(2).map(|at| u8::from_str_radix(&text[at..at + 2], 16).ok()).collect()
}
