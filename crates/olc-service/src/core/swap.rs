//! A coin swap between two coins with nobody in the middle, as in Bitcoin's atomic swaps (BIP
//! 199). The maker locks coins on one chain under a hash lock that the taker opens with a secret
//! and the maker takes back after a height. The taker then locks on the other chain under the same
//! hash, for the maker, with an earlier way back. The maker takes it with the secret, which puts the
//! secret in that chain, and the taker reads it there and takes the maker's coins. Each side uses
//! the same key on both coins and passes one text to the other, and the nodes do the rest.
use olc_core::coin::CoinConfig;
use olc_core::{Block, Hash, OutPoint, Tx};
use olc_crypto::hash;
use olc_lang::words::{lock_from_text, parse_words, words_of, Dialect, Word};
use olc_lang::BETA;
use olc_node::locks::LockFailure;
use olc_node::Engine;
use olc_wallet::format_address;
use serde::{Deserialize, Serialize};

const OFFER_START: &str = "olcswap1";
const ACCEPT_START: &str = "olcswapok1";
/// Blocks the shortest wait may be, so a slow block or two cannot eat it.
const FEWEST_BLOCKS: u64 = 6;
/// How far back a node looks for the payment that opened a lock.
const MOST_BLOCKS_SEARCHED: u64 = 2_000;
/// How far back a node looks for a payment of its own it waits on.
const RECENT_BLOCKS: u64 = 60;
/// Blocks a maker leaves before the taker's way back opens.
const CLAIM_MARGIN: u64 = 2;

/// One coin of a swap and how much of it changes hands. The coin goes whole, so the other side
/// can run a node on it with nothing more than the text.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Side {
    #[serde(with = "coin_as_file")]
    pub coin: CoinConfig,
    pub units: u64,
}

/// A coin in a swap text goes as its coin file. The binary form leaves out the rules older
/// builds could not read, and a coin read back from it would be another coin. The built-in coin
/// every build carries goes as nothing, which keeps the text and its QR code short.
mod coin_as_file {
    use olc_core::coin::CoinConfig;
    use olc_store::coin_file::{from_json, to_json};
    use serde::{de, ser, Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(coin: &CoinConfig, writer: S) -> Result<S::Ok, S::Error> {
        if *coin == CoinConfig::olc() {
            return writer.serialize_str("");
        }
        let file: serde_json::Value = serde_json::from_str(&to_json(coin).map_err(ser::Error::custom)?).map_err(ser::Error::custom)?;
        writer.serialize_str(&file.to_string())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(reader: D) -> Result<CoinConfig, D::Error> {
        match String::deserialize(reader)? {
            built_in if built_in.is_empty() => Ok(CoinConfig::olc()),
            file => from_json(&file).map_err(de::Error::custom),
        }
    }
}

/// What the maker offers: `give` on its coin for `take` on the other.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Offer {
    pub give: Side,
    pub take: Side,
    /// The start of the secret's hash, as the HASH word gives it.
    pub hash: [u8; 20],
    pub maker: [u8; 20],
    /// The height on the maker's coin from which the maker may take its coins back.
    pub refund_at: u64,
    /// How long the taker's lock waits, in minutes. The maker's waits twice as long.
    pub wait_minutes: u32,
}

/// What the taker answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Accept {
    pub offer: Hash,
    pub taker: [u8; 20],
    /// The height on the taker's coin from which the taker may take its coins back.
    pub refund_at: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    Maker,
    Taker,
}

/// How a swap ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Outcome {
    Swapped,
    Refunded,
    Cancelled,
    /// The maker's lock came too close to its way back for the taker to lock safely.
    TooLate,
}

/// One swap as this wallet keeps it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Swap {
    pub role: Role,
    pub offer: Offer,
    pub accept: Option<Accept>,
    /// Hex. The maker has it from the start, the taker once the maker used it.
    pub secret: Option<String>,
    pub my_lock: Option<Hash>,
    pub my_point: Option<OutPoint>,
    pub my_claim: Option<Hash>,
    pub my_refund: Option<Hash>,
    pub outcome: Option<Outcome>,
}

/// What a step did, for a person to hear.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Happened {
    Locked,
    Claimed,
    Refunded,
    Swapped,
    /// The coins taken back are in a block.
    CameBack,
    TooLate,
}

/// Where a swap stands, for a screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// The maker waits for the taker's answer text.
    WaitingForAnswer,
    /// The taker waits for the maker's coins to be locked.
    WaitingForTheirLock,
    /// This side's coins are locked and wait for the other side.
    Locked,
    /// This side took the other's coins and waits for a block.
    Claiming,
    /// The way back opened and this side took its coins back.
    Refunding,
    Ended(Outcome),
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn from_hex(text: &str) -> Option<Vec<u8>> {
    let text = text.trim();
    let digit = |at: usize| u8::from_str_radix(text.get(at..at + 2)?, 16).ok();
    (text.len().is_multiple_of(2)).then(|| (0..text.len()).step_by(2).map(digit).collect())?
}

fn text_of<T: Serialize>(start: &str, value: &T) -> String {
    format!("{start}.{}", hex(&postcard::to_allocvec(value).unwrap_or_default()))
}

fn read_text<T: for<'a> Deserialize<'a>>(start: &str, text: &str) -> Option<T> {
    let (head, body) = text.trim().split_once('.')?;
    head.eq_ignore_ascii_case(start).then_some(())?;
    postcard::from_bytes(&from_hex(body)?).ok()
}

impl Offer {
    pub fn id(&self) -> Hash {
        hash(&postcard::to_allocvec(self).unwrap_or_default())
    }

    pub fn to_text(&self) -> String {
        text_of(OFFER_START, self)
    }

    pub fn from_text(text: &str) -> Option<Self> {
        read_text(OFFER_START, text)
    }
}

impl Accept {
    pub fn to_text(&self) -> String {
        text_of(ACCEPT_START, self)
    }

    pub fn from_text(text: &str) -> Option<Self> {
        read_text(ACCEPT_START, text)
    }
}

/// Blocks a coin makes in `minutes`, never fewer than [`FEWEST_BLOCKS`].
pub fn blocks_in(engine: &Engine, minutes: u32) -> u64 {
    let block_ms = engine.chain().config().rules.block_time_ms.max(1);
    (u64::from(minutes) * 60_000).div_ceil(block_ms).max(FEWEST_BLOCKS)
}

/// The beta words of a hash lock with a way back: `claimer` opens it with the secret and its
/// key, and `refunder` with its key from `refund_at` on.
pub fn swap_lock(hash: &[u8; 20], claimer: &[u8; 20], refunder: &[u8; 20], refund_at: u64) -> String {
    let (hash, claimer, refunder) = (hex(hash), hex(claimer), hex(refunder));
    format!(
        "IF HASH 0x{hash} EQUALVERIFY DUP HASH 0x{claimer} EQUALVERIFY CHECKSIG \
         ELSE {refund_at} CHECKHEIGHT DROP DUP HASH 0x{refunder} EQUALVERIFY CHECKSIG ENDIF"
    )
}

/// The secret in an unlock that opened a swap lock, if it is there.
pub fn secret_in(unlock: &[u8], hash20: &[u8; 20]) -> Option<Vec<u8>> {
    let words = parse_words(&words_of(unlock)).ok()?;
    words.into_iter().find_map(|word| match word {
        Word::Data(data) if hash(&data)[..20] == hash20[..] => Some(data),
        _ => None,
    })
}

/// A new secret and the offer for it, with the maker's coins locked until `wait_minutes` twice.
pub fn make_offer(maker: &Engine, give: Side, take: Side, wait_minutes: u32) -> Result<Swap, String> {
    let mut secret = [0u8; 32];
    getrandom::fill(&mut secret).map_err(|error| error.to_string())?;
    let refund_at = maker.chain().height() + 2 * blocks_in(maker, wait_minutes);
    let hash20: [u8; 20] = hash(&secret)[..20].try_into().map_err(|_| "hash".to_string())?;
    let offer = Offer { give, take, hash: hash20, maker: maker.wallet().address(), refund_at, wait_minutes };
    Ok(Swap::new(Role::Maker, offer, Some(hex(&secret))))
}

/// Takes an offer: the answer names this wallet's key and when its own lock opens again.
pub fn take_offer(offer: Offer, taker: &Engine) -> Swap {
    let refund_at = taker.chain().height() + blocks_in(taker, offer.wait_minutes);
    let accept = Accept { offer: offer.id(), taker: taker.wallet().address(), refund_at };
    let mut swap = Swap::new(Role::Taker, offer, None);
    swap.accept = Some(accept);
    swap
}

impl Swap {
    fn new(role: Role, offer: Offer, secret: Option<String>) -> Self {
        Self {
            role,
            offer,
            accept: None,
            secret,
            my_lock: None,
            my_point: None,
            my_claim: None,
            my_refund: None,
            outcome: None,
        }
    }

    pub fn id(&self) -> Hash {
        self.offer.id()
    }

    /// The text this side hands to the other: the offer, or the answer to it.
    pub fn text(&self) -> String {
        match (&self.role, &self.accept) {
            (Role::Taker, Some(accept)) => accept.to_text(),
            _ => self.offer.to_text(),
        }
    }

    /// The coin this side pays with and the coin it gets.
    pub fn coins(&self) -> (&Side, &Side) {
        match self.role {
            Role::Maker => (&self.offer.give, &self.offer.take),
            Role::Taker => (&self.offer.take, &self.offer.give),
        }
    }

    fn keys(&self) -> Option<([u8; 20], [u8; 20], u64, u64)> {
        let accept = self.accept.as_ref()?;
        Some((self.offer.maker, accept.taker, self.offer.refund_at, accept.refund_at))
    }

    /// The lock on the maker's coin and the lock on the taker's coin.
    fn locks(&self) -> Option<(String, String)> {
        let (maker, taker, maker_refund, taker_refund) = self.keys()?;
        let hash = &self.offer.hash;
        Some((swap_lock(hash, &taker, &maker, maker_refund), swap_lock(hash, &maker, &taker, taker_refund)))
    }

    /// This side's lock and the other side's, as words.
    pub fn my_and_their_lock(&self) -> Option<(String, String)> {
        let (maker_lock, taker_lock) = self.locks()?;
        Some(match self.role {
            Role::Maker => (maker_lock, taker_lock),
            Role::Taker => (taker_lock, maker_lock),
        })
    }

    /// The height this side's coins come back from, on the coin it pays with.
    pub fn my_refund_at(&self) -> Option<u64> {
        let (_, _, maker_refund, taker_refund) = self.keys()?;
        Some(match self.role {
            Role::Maker => maker_refund,
            Role::Taker => taker_refund,
        })
    }

    pub fn me(&self) -> Option<[u8; 20]> {
        let (maker, taker, ..) = self.keys()?;
        Some(match self.role {
            Role::Maker => maker,
            Role::Taker => taker,
        })
    }

    /// Ends a swap before any coin of this side is locked. After that only time ends it.
    pub fn cancel(&mut self) -> bool {
        let free = self.my_lock.is_none() && self.outcome.is_none();
        if free {
            self.outcome = Some(Outcome::Cancelled);
        }
        free
    }

    pub fn stage(&self) -> Stage {
        if let Some(outcome) = self.outcome {
            return Stage::Ended(outcome);
        }
        match (self.role, &self.accept, self.my_lock, self.my_claim, self.my_refund) {
            (_, _, _, _, Some(_)) => Stage::Refunding,
            (_, _, _, Some(_), None) => Stage::Claiming,
            (_, _, Some(_), None, None) => Stage::Locked,
            (Role::Maker, None, None, None, None) => Stage::WaitingForAnswer,
            _ => Stage::WaitingForTheirLock,
        }
    }

    /// Moves the swap on as far as the two chains allow. `mine` runs the coin this side pays
    /// with and `theirs` the coin it gets.
    pub fn step(&mut self, mine: &mut Engine, theirs: &mut Engine) -> Option<Happened> {
        if self.outcome.is_some() {
            return None;
        }
        let (my_words, their_words) = self.my_and_their_lock()?;
        let my_lock = lock_from_text(&my_words).ok()?.0;
        let their_lock = lock_from_text(&their_words).ok()?.0;
        let (giving, getting) = (self.coins().0.units, self.coins().1.units);
        if self.my_point.is_none() {
            self.my_point = self.my_lock.and_then(|txid| find_lock(mine, &my_lock, giving, Some(txid)));
        }
        if self.role == Role::Taker && self.secret.is_none() {
            self.secret = self.my_point.and_then(|point| spender_secret(mine, &point, &self.offer.hash));
        }
        if let Some(txid) = self.my_refund {
            if in_chain(mine, &txid) {
                self.outcome = Some(Outcome::Refunded);
                return Some(Happened::CameBack);
            }
            // A taker whose coins the maker took first, with the secret, takes the maker's.
            if self.role == Role::Maker || self.secret.is_none() {
                return None;
            }
        }
        if self.my_claim.is_none() && self.my_refund.is_none() {
            if let Some(happened) = self.refund(mine) {
                return Some(happened);
            }
        }
        if self.my_lock.is_none() {
            return self.lock(mine, theirs, &my_words, &their_lock, giving);
        }
        if let Some(txid) = self.my_claim {
            return in_chain(theirs, &txid).then(|| {
                self.outcome = Some(Outcome::Swapped);
                Happened::Swapped
            });
        }
        self.claim(theirs, &their_lock, getting)
    }

    /// Takes the other side's coins with the secret. A maker stops short of the taker's way back,
    /// since the secret it shows then would let the taker take both.
    fn claim(&mut self, theirs: &mut Engine, their_lock: &[u8], units: u64) -> Option<Happened> {
        let secret = self.secret.clone()?;
        let (.., taker_refund) = self.keys()?;
        if self.role == Role::Maker && theirs.chain().height() + CLAIM_MARGIN >= taker_refund {
            return None;
        }
        let point = find_lock(theirs, their_lock, units, None)?;
        let me = format_address(&self.me()?);
        let unlock = format!("sig:{me} key:{me} 0x{secret} 1");
        let spend = theirs.spend_lock(point, self.me()?, &unlock).ok()?;
        self.my_claim = Some(theirs.finish_lock(spend).ok()?);
        Some(Happened::Claimed)
    }

    fn lock(&mut self, mine: &mut Engine, theirs: &Engine, words: &str, their_lock: &[u8], units: u64) -> Option<Happened> {
        if self.role == Role::Taker {
            find_lock(theirs, their_lock, self.coins().1.units, None)?;
            let left = self.offer.refund_at.saturating_sub(theirs.chain().height());
            if left < blocks_in(theirs, self.offer.wait_minutes) {
                self.outcome = Some(Outcome::TooLate);
                return Some(Happened::TooLate);
            }
        }
        let (txid, _) = mine.pay_to_lock_in(Dialect::Beta, words, units).map_err(|_: LockFailure| ()).ok()?;
        self.my_lock = Some(txid);
        Some(Happened::Locked)
    }

    /// Takes this side's coins back once its lock opens for it and nobody took them.
    fn refund(&mut self, mine: &mut Engine) -> Option<Happened> {
        let point = self.my_point?;
        if mine.chain().height() + 1 < self.my_refund_at()? {
            return None;
        }
        mine.chain().coin_at(&point)?;
        let me = format_address(&self.me()?);
        let spend = mine.spend_lock(point, self.me()?, &format!("sig:{me} key:{me} 0")).ok()?;
        self.my_refund = Some(mine.finish_lock(spend).ok()?);
        Some(Happened::Refunded)
    }
}

/// The coin under `lock` worth at least `units`, paid by `txid` when that is given.
fn find_lock(engine: &Engine, lock: &[u8], units: u64, txid: Option<Hash>) -> Option<OutPoint> {
    let mut found = engine.chain().coins_under(BETA).filter(|(point, entry)| {
        entry.output.lock == lock && entry.output.value >= units && txid.is_none_or(|txid| point.txid == txid)
    });
    found.next().map(|(point, _)| *point)
}

fn recent_blocks(engine: &Engine, most: u64) -> Vec<Block> {
    let first = engine.chain().height().saturating_sub(most.saturating_sub(1)).max(1);
    engine.chain().blocks_from(first, usize::try_from(most).unwrap_or(usize::MAX))
}

/// Whether `txid` is in a block: its first coin is still there, or a recent block has it.
fn in_chain(engine: &Engine, txid: &Hash) -> bool {
    let first = OutPoint { txid: *txid, index: 0 };
    let mut recent = recent_blocks(engine, RECENT_BLOCKS).into_iter().flat_map(|block| block.txs);
    engine.chain().coin_at(&first).is_some() || recent.any(|tx| tx.id() == *txid)
}

/// The secret in whatever spent `point`, waiting or in a block.
fn spender_secret(engine: &Engine, point: &OutPoint, hash20: &[u8; 20]) -> Option<String> {
    let spends = |tx: &Tx| tx.inputs.iter().find(|input| input.previous == *point).map(|input| input.unlock.clone());
    let waiting = engine.pool().iter().find_map(spends);
    let unlock = match waiting {
        Some(unlock) => unlock,
        // Still unspent: nobody used the secret yet, and no block needs reading.
        None if engine.chain().coin_at(point).is_some() => return None,
        None => recent_blocks(engine, MOST_BLOCKS_SEARCHED).iter().flat_map(|block| &block.txs).find_map(spends)?,
    };
    secret_in(&unlock, hash20).map(|secret| hex(&secret))
}

#[cfg(test)]
#[path = "swap_tests.rs"]
mod tests;
