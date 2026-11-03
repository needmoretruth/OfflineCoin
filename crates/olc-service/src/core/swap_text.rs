//! A swap offer as text. Each coin goes by its id and ticker, so the text and its QR code stay
//! short, beside the addresses of the maker's node. The taker's node fetches a coin it lacks
//! there and keeps it only when it hashes to the id the text names.
use super::swap::{read_text, text_of, Offer, Side};
use olc_core::coin::CoinConfig;
use olc_core::Hash;
use olc_crypto::hash;
use olc_store::coin_file::to_json;
use serde::{Deserialize, Serialize};

const OFFER_START: &str = "olcswap1";
/// The most addresses a swap text carries. A text with more is not from this app, and asking
/// each would hold the taker up.
pub const MOST_ADDRESSES: usize = 8;

/// A coin's id: the hash of its coin file. A fork keeps the genesis block of the chain it leaves
/// and differs by its rules, so the id covers the whole file.
pub fn coin_id(coin: &CoinConfig) -> Hash {
    hash(to_json(coin).unwrap_or_default().as_bytes())
}

/// One side of an offer as the text carries it: the coin by id and ticker, and how much.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Named {
    pub id: Hash,
    pub ticker: String,
    pub units: u64,
}

impl Named {
    fn of(side: &Side) -> Self {
        Self { id: coin_id(&side.coin), ticker: side.coin.ticker.clone(), units: side.units }
    }

    /// Whether `coin` is the coin this side names.
    pub fn is(&self, coin: &CoinConfig) -> bool {
        coin.ticker == self.ticker && coin_id(coin) == self.id
    }
}

/// An offer as the text carries it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OfferText {
    pub give: Named,
    pub take: Named,
    pub hash: [u8; 20],
    pub maker: [u8; 20],
    pub refund_at: u64,
    pub wait_minutes: u32,
    pub at: Vec<String>,
}

impl OfferText {
    pub fn from_text(text: &str) -> Option<Self> {
        let read: Self = read_text(OFFER_START, text)?;
        (read.at.len() <= MOST_ADDRESSES).then_some(read)
    }

    pub fn to_text(&self) -> String {
        text_of(OFFER_START, self)
    }

    /// The swap's id: the hash of the offer as both sides read it.
    pub fn id(&self) -> Hash {
        hash(&postcard::to_allocvec(self).unwrap_or_default())
    }

    /// The offer, once both its coins are at hand. `None` when either is not the coin named.
    pub fn with_coins(self, give: CoinConfig, take: CoinConfig) -> Option<Offer> {
        if !self.give.is(&give) || !self.take.is(&take) {
            return None;
        }
        Some(Offer {
            give: Side { coin: give, units: self.give.units },
            take: Side { coin: take, units: self.take.units },
            hash: self.hash,
            maker: self.maker,
            refund_at: self.refund_at,
            wait_minutes: self.wait_minutes,
            at: self.at,
        })
    }
}

impl Offer {
    pub fn as_text(&self) -> OfferText {
        OfferText {
            give: Named::of(&self.give),
            take: Named::of(&self.take),
            hash: self.hash,
            maker: self.maker,
            refund_at: self.refund_at,
            wait_minutes: self.wait_minutes,
            at: self.at.clone(),
        }
    }

    pub fn id(&self) -> Hash {
        self.as_text().id()
    }

    pub fn to_text(&self) -> String {
        self.as_text().to_text()
    }
}

/// The coin `named`: one at hand, or else the coin a node at one of `at` runs. Whatever answers
/// there may send any coin, so only one that hashes to the named id counts.
pub fn find_coin(
    named: &Named,
    at_hand: impl IntoIterator<Item = CoinConfig>,
    at: &[String],
    ask: impl Fn(&str) -> Option<CoinConfig>,
) -> Option<CoinConfig> {
    if let Some(coin) = at_hand.into_iter().find(|coin| named.is(coin)) {
        return Some(coin);
    }
    at.iter().take(MOST_ADDRESSES).filter_map(|address| ask(address)).find(|coin| named.is(coin))
}
