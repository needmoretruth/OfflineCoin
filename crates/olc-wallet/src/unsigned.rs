//! A payment nobody signed yet, as it travels to a wallet that may never touch a network.
use alloc::vec::Vec;
use core::fmt;
use olc_core::{Address, Tx};
use olc_crypto::Hash;
use serde::de::{SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// The payment, the address each input spends from, and the coin its signatures name. A wallet
/// with no chain cannot know which coin that is, so it comes along.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unsigned {
    pub tx: Tx,
    pub owners: Vec<Address>,
    /// See [`olc_core::CoinConfig::signing_id_at`]. `None` below the coin's switch height.
    pub coin: Option<Hash>,
}

impl Serialize for Unsigned {
    /// Without a coin it is the pair older builds wrote and read.
    fn serialize<S: Serializer>(&self, writer: S) -> Result<S::Ok, S::Error> {
        match &self.coin {
            None => (&self.tx, &self.owners).serialize(writer),
            Some(coin) => (&self.tx, &self.owners, coin).serialize(writer),
        }
    }
}

struct Fields;

impl<'de> Visitor<'de> for Fields {
    type Value = Unsigned;

    fn expecting(&self, out: &mut fmt::Formatter) -> fmt::Result {
        out.write_str("a payment, its owners and maybe its coin")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut fields: A) -> Result<Unsigned, A::Error> {
        let missing = || serde::de::Error::custom("a payment and its owners");
        let tx = fields.next_element()?.ok_or_else(missing)?;
        let owners = fields.next_element()?.ok_or_else(missing)?;
        let coin = fields.next_element()?;
        Ok(Unsigned { tx, owners, coin })
    }
}

impl<'de> Deserialize<'de> for Unsigned {
    fn deserialize<D: Deserializer<'de>>(reader: D) -> Result<Self, D::Error> {
        reader.deserialize_seq(Fields)
    }
}
