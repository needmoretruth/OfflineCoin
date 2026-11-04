//! The Lab's bug exploiter: one material of the Lab screen. It makes real invalid blocks and
//! payments on a practice coin, sends each to a running node over the network exactly as a peer
//! would, and reports that node's own refusal and the rule it named. Nothing here is a
//! simulation: every block carries real proof of work at the practice coin's difficulty (save the
//! "too little work" trick, whose whole point is that it does not), every refusal is read from the
//! receiving node's own `BlockRejected`/`TxRejected` event, never decided on the sender's side.
//!
//! # How the Lab calls it
//!
//! ```ignore
//! use olc_service::local::trouble::{try_trick, Trick, Outcome};
//! // `verdicts` yields the node's newest verdicts from its status line, e.g.
//! //   || supervisor.status(node_id).map(|s| s.verdicts).unwrap_or_default()
//! for trick in Trick::ALL {
//!     let out: Outcome = try_trick(trick, &coin, addr, &wallet_folder, node_name, &verdicts)?;
//!     // out.refused is true, out.reason is the rule the node named (e.g. "BadCoinbase"),
//!     // out.by is which node refused it. Show trick.key() and trick.rule_key() as sentences.
//! }
//! ```
//!
//! `try_trick(trick, coin, target, funds, by, verdicts)`: builds the trick on a fresh copy of the
//! node's chain (synced from the node over the network), sends it, and waits for the node to say
//! what it did. `funds` is a wallet folder holding spendable coins on `coin`; it is read, never
//! written. `verdicts` returns the node's current verdict list, which the supervisor reads from
//! the node's status line.
#[path = "trouble_peer.rs"]
mod trouble_peer;

use crate::core::Verdict;
use trouble_peer::Peer;
use olc_core::coin::COIN;
use olc_core::types::{merkle_root, version_for};
use olc_core::{
    Block, Chain, ChainError, CoinConfig, OutPoint, Tx, TxIn, TxOut, SEQUENCE_FINAL,
};
use olc_crypto::{address_of, public_key, sign};
use std::net::SocketAddr;
use std::path::Path;
use std::time::{Duration, Instant};

/// How long one trick waits for the node to say what it did before giving up.
const VERDICT_WAIT: Duration = Duration::from_secs(20);
const POLL: Duration = Duration::from_millis(150);
/// Attempts per sealing batch, and how many batches before a seal is called hopeless.
const SEAL_BATCH: u32 = 80_000;
const SEAL_BATCHES: u32 = 4_000;
/// A block timed this far past the node's clock, well over Bitcoin's two-hour-style window that
/// the practice coins set to thirty seconds.
const FAR_AHEAD_MS: u64 = 3_600_000;
/// Bytes of junk data that carry a block over the practice coin's block-size limit.
const OVERSIZE_DATA: usize = 8 * 1024;

/// One real consensus violation the exploiter can commit. [`Trick::ALL`] lists them in an order
/// that teaches: the money rules first (a miner paying itself too much, double spending, spending
/// what is not there or not yours, spending a reward too early), then the block rules (a clock too
/// far ahead, too little work, a block too big).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trick {
    /// A block whose mining reward is larger than the rules allow.
    ExtraReward,
    /// One payment that spends the same coin twice.
    DoubleSpend,
    /// A payment that spends a coin that was never made.
    SpendMissing,
    /// A payment that spends someone else's coin with a made-up signature.
    ForgedSignature,
    /// A payment that spends a mining reward before it is old enough.
    ImmatureReward,
    /// A block stamped far ahead of the node's clock.
    FutureBlock,
    /// A block that carries too little work for its difficulty.
    TooLittleWork,
    /// A block larger than the size limit.
    OversizeBlock,
}

impl Trick {
    /// Every trick, in a good teaching order.
    pub const ALL: [Trick; 8] = [
        Trick::ExtraReward,
        Trick::DoubleSpend,
        Trick::SpendMissing,
        Trick::ForgedSignature,
        Trick::ImmatureReward,
        Trick::FutureBlock,
        Trick::TooLittleWork,
        Trick::OversizeBlock,
    ];

    /// The i18n stem for the one-line "what was tried" sentence.
    pub fn key(self) -> &'static str {
        match self {
            Trick::ExtraReward => "trouble.extra_reward",
            Trick::DoubleSpend => "trouble.double_spend",
            Trick::SpendMissing => "trouble.spend_missing",
            Trick::ForgedSignature => "trouble.forged_signature",
            Trick::ImmatureReward => "trouble.immature_reward",
            Trick::FutureBlock => "trouble.future_block",
            Trick::TooLittleWork => "trouble.too_little_work",
            Trick::OversizeBlock => "trouble.oversize_block",
        }
    }

    /// The i18n key for the one-line "which rule stopped it" sentence.
    pub fn rule_key(self) -> &'static str {
        match self {
            Trick::ExtraReward => "trouble.extra_reward_rule",
            Trick::DoubleSpend => "trouble.double_spend_rule",
            Trick::SpendMissing => "trouble.spend_missing_rule",
            Trick::ForgedSignature => "trouble.forged_signature_rule",
            Trick::ImmatureReward => "trouble.immature_reward_rule",
            Trick::FutureBlock => "trouble.future_block_rule",
            Trick::TooLittleWork => "trouble.too_little_work_rule",
            Trick::OversizeBlock => "trouble.oversize_block_rule",
        }
    }

    /// The rule the honest node is expected to name, as its [`ChainError`] prints. The exploiter
    /// checks the node's answer against this: a mismatch, or no refusal at all, is a consensus bug.
    pub fn expected(self) -> ChainError {
        match self {
            Trick::ExtraReward => ChainError::BadCoinbase,
            Trick::DoubleSpend => ChainError::DuplicateInput,
            Trick::SpendMissing => ChainError::MissingInput,
            Trick::ForgedSignature => ChainError::BadLock,
            Trick::ImmatureReward => ChainError::ImmatureCoinbase,
            Trick::FutureBlock => ChainError::BadTime,
            Trick::TooLittleWork => ChainError::BadSeal,
            Trick::OversizeBlock => ChainError::TooLarge,
        }
    }

    /// True for a trick carried in a block, false for one carried in a payment.
    pub fn is_block(self) -> bool {
        matches!(self, Trick::ExtraReward | Trick::FutureBlock | Trick::TooLittleWork | Trick::OversizeBlock)
    }
}

/// What one node did with one trick.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub trick: Trick,
    /// True when the node refused it, as it must.
    pub refused: bool,
    /// The rule the node named, as its [`ChainError`] prints (for example `"BadCoinbase"`), or
    /// nothing when the node took it.
    pub reason: Option<String>,
    /// The node that answered.
    pub by: String,
}

impl Outcome {
    /// True when the node refused the trick with the very rule it should have.
    pub fn refused_as_expected(&self) -> bool {
        self.refused && self.reason.as_deref() == Some(expected_name(self.trick.expected()))
    }
}

/// A valid payment, sent the same way, so a caller can tell a real refusal from a broken sender.
pub fn valid_payment(
    coin: &CoinConfig,
    target: SocketAddr,
    funds: &Path,
    by: &str,
    verdicts: &dyn Fn() -> Vec<Verdict>,
) -> Result<Outcome, String> {
    let mut peer = Peer::connect(coin, target)?;
    peer.catch_up()?;
    let tx = a_valid_payment(&peer.chain, funds)?;
    send_tx(&mut peer, tx, by, verdicts, Trick::DoubleSpend)
}

/// A valid block, sealed and sent the same way, which the node must take.
pub fn valid_block(
    coin: &CoinConfig,
    target: SocketAddr,
    _funds: &Path,
    by: &str,
    verdicts: &dyn Fn() -> Vec<Verdict>,
) -> Result<Outcome, String> {
    let mut peer = Peer::connect(coin, target)?;
    peer.catch_up()?;
    let miner = address_of(&throwaway_key().1);
    let mut block = peer.chain.block_template(&miner, &[], now_ms());
    reseal(&peer.chain, &mut block)?;
    send_block(&mut peer, block, by, verdicts, Trick::ExtraReward)
}

/// Commits `trick` against the node at `target` and returns what the node did about it.
pub fn try_trick(
    trick: Trick,
    coin: &CoinConfig,
    target: SocketAddr,
    funds: &Path,
    by: &str,
    verdicts: &dyn Fn() -> Vec<Verdict>,
) -> Result<Outcome, String> {
    let mut peer = Peer::connect(coin, target)?;
    peer.catch_up()?;
    if trick.is_block() {
        let block = build_block(trick, &peer.chain)?;
        send_block(&mut peer, block, by, verdicts, trick)
    } else {
        let tx = build_tx(trick, coin, &peer.chain, funds)?;
        send_tx(&mut peer, tx, by, verdicts, trick)
    }
}

// --- sending and reading the node's verdict -----------------------------------------------------

fn send_block(
    peer: &mut Peer,
    block: Block,
    by: &str,
    verdicts: &dyn Fn() -> Vec<Verdict>,
    trick: Trick,
) -> Result<Outcome, String> {
    let baseline = newest(verdicts);
    peer.send(&olc_node::Message::Block(block))?;
    await_verdict(verdicts, baseline, true, by, trick)
}

fn send_tx(
    peer: &mut Peer,
    tx: Tx,
    by: &str,
    verdicts: &dyn Fn() -> Vec<Verdict>,
    trick: Trick,
) -> Result<Outcome, String> {
    let baseline = newest(verdicts);
    peer.send(&olc_node::Message::Tx(tx))?;
    await_verdict(verdicts, baseline, false, by, trick)
}

fn newest(verdicts: &dyn Fn() -> Vec<Verdict>) -> u64 {
    verdicts().iter().map(|verdict| verdict.number).max().unwrap_or(0)
}

/// Waits for the node's next verdict of the right kind (a block's or a payment's) and reads from it
/// alone what the node did.
fn await_verdict(
    verdicts: &dyn Fn() -> Vec<Verdict>,
    baseline: u64,
    block: bool,
    by: &str,
    trick: Trick,
) -> Result<Outcome, String> {
    let until = Instant::now() + VERDICT_WAIT;
    loop {
        let fresh = verdicts().into_iter().filter(|verdict| verdict.number > baseline && verdict.block == block);
        if let Some(verdict) = fresh.min_by_key(|verdict| verdict.number) {
            return Ok(Outcome { trick, refused: verdict.refused.is_some(), reason: verdict.refused, by: by.to_string() });
        }
        if Instant::now() > until {
            return Err(format!("{by} said nothing about the {} sent to it.", if block { "block" } else { "payment" }));
        }
        std::thread::sleep(POLL);
    }
}

// --- building the invalid blocks ----------------------------------------------------------------

fn build_block(trick: Trick, chain: &Chain) -> Result<Block, String> {
    let miner = address_of(&throwaway_key().1);
    let mut block = chain.block_template(&miner, &[], now_ms());
    match trick {
        Trick::ExtraReward => {
            // Claim one coin more than the reward and fees allow.
            let reward = block.txs[0].outputs.get_mut(0).ok_or("the coinbase has no reward output.")?;
            reward.value = reward.value.saturating_add(COIN);
            block.header.merkle_root = merkle_root(&block.txs);
            reseal(chain, &mut block)?;
        }
        Trick::OversizeBlock => {
            // Carry junk data past the block-size limit. The reward output stays honest.
            block.txs[0].outputs.push(TxOut::data(vec![0x7au8; OVERSIZE_DATA]));
            block.header.merkle_root = merkle_root(&block.txs);
            reseal(chain, &mut block)?;
            let limit = chain.rules().max_block_bytes;
            if (block.size() as u64) <= limit {
                return Err(format!("the block is not over the {limit}-byte limit."));
            }
        }
        Trick::FutureBlock => {
            // A clock far ahead of the node's. Bitcoin's header carries whole seconds.
            block.header.time_ms = (now_ms().saturating_add(FAR_AHEAD_MS)) / 1_000 * 1_000;
            reseal(chain, &mut block)?;
        }
        Trick::TooLittleWork => {
            // The one block with no real work: a nonce that does not meet the difficulty.
            let consensus = &chain.plugins().consensus;
            while consensus.check_seal(&block.header) {
                block.header.nonce = block.header.nonce.wrapping_add(1);
            }
        }
        _ => return Err("not a block trick.".into()),
    }
    Ok(block)
}

/// Seals a tampered block at the chain's difficulty, so the node refuses it for its rule, not for
/// its work.
fn reseal(chain: &Chain, block: &mut Block) -> Result<(), String> {
    let consensus = &chain.plugins().consensus;
    for _ in 0..SEAL_BATCHES {
        if consensus.try_seal(&mut block.header, SEAL_BATCH) {
            return Ok(());
        }
    }
    Err("could not seal the block at the practice difficulty.".into())
}

// --- building the invalid payments --------------------------------------------------------------

fn build_tx(trick: Trick, coin: &CoinConfig, chain: &Chain, funds: &Path) -> Result<Tx, String> {
    match trick {
        Trick::DoubleSpend => {
            // One owned coin named by two inputs, both signed, so the node reaches the rule that a
            // coin is spent once (not a broken-signature rule): the duplicate must carry a valid
            // signature, so it is signed in place, not copied from a one-input payment.
            let wallet = olc_store::load_wallet(funds)?;
            let (owner, point, value) = owned_coin(&wallet, chain).ok_or("the funds wallet has no spendable coin.")?;
            let next = chain.height() + 1;
            let signing = chain.config().signing_id_at(next);
            let version = version_for(signing.as_ref());
            let to = wallet.address();
            let input = || TxIn { previous: point, unlock: Vec::new(), sequence: SEQUENCE_FINAL };
            let mut tx = Tx {
                version,
                inputs: vec![input(), input()],
                outputs: vec![TxOut { value: value / 2, language: 0, lock: to.to_vec() }],
                coinbase_height: None,
                lock_time: 0,
            };
            let owners = [owner, owner];
            for index in 0..owners.len() {
                wallet
                    .sign_input(&mut tx, &owners, index, signing.as_ref())
                    .map_err(|error| format!("could not sign the double spend: {error:?}"))?;
            }
            Ok(tx)
        }
        Trick::SpendMissing => {
            // Spend a coin that was never made.
            let nothing = OutPoint { txid: [0x33u8; 32], index: 0 };
            Ok(one_input_tx(coin, chain, nothing, Vec::new()))
        }
        Trick::ForgedSignature => {
            // Spend a real mature coin with a made-up signature from a key that does not own it.
            let point = mature_coin(chain).ok_or("the chain holds no mature coin to forge against.")?;
            let mut tx = one_input_tx(coin, chain, point, Vec::new());
            let (secret, public) = throwaway_key();
            let signing = chain.config().signing_id_at(chain.height() + 1);
            let digest = tx.signature_hash(0, address_of(&public).as_slice(), signing.as_ref());
            let signature = sign(&secret, &digest).ok_or("could not sign the forgery.")?;
            tx.inputs[0].unlock = olc_lang::alpha::unlock_data(&public, &signature);
            Ok(tx)
        }
        Trick::ImmatureReward => {
            // Spend the newest mining reward, which is too young to spend.
            let point = immature_coin(chain).ok_or("the chain holds no reward young enough to be immature.")?;
            Ok(one_input_tx(coin, chain, point, Vec::new()))
        }
        _ => Err("not a payment trick.".into()),
    }
}

/// A valid signed payment that spends the funds wallet's coins back to itself.
fn a_valid_payment(chain: &Chain, funds: &Path) -> Result<Tx, String> {
    let wallet = olc_store::load_wallet(funds)?;
    let to = wallet.address();
    let amount = COIN / 10;
    wallet.pay(chain, to, amount, 0, &[]).map_err(|error| format!("the funds wallet could not pay: {error:?}"))
}

/// A one-input payment that spends `point`, with the right version for the coin.
fn one_input_tx(coin: &CoinConfig, chain: &Chain, point: OutPoint, unlock: Vec<u8>) -> Tx {
    let next = chain.height() + 1;
    let version = version_for(coin.signing_id_at(next).as_ref());
    let value = chain.coin_at(&point).map_or(COIN / 100, |coin| coin.output.value.saturating_sub(1));
    let to = address_of(&throwaway_key().1);
    Tx {
        version,
        inputs: vec![TxIn { previous: point, unlock, sequence: SEQUENCE_FINAL }],
        outputs: vec![TxOut { value, language: 0, lock: to.to_vec() }],
        coinbase_height: None,
        lock_time: 0,
    }
}

/// One mature coin the funds wallet owns, as (owner, outpoint, value).
fn owned_coin(wallet: &olc_wallet::Wallet, chain: &Chain) -> Option<(olc_core::Address, OutPoint, u64)> {
    let next = chain.height() + 1;
    for (owner, _) in wallet.keys() {
        for (point, entry) in chain.unspent_of(&owner) {
            if chain.is_mature(&entry, next) {
                return Some((owner, point, entry.output.value));
            }
        }
    }
    None
}

/// A mature, unspent mining reward on the chain, oldest first.
fn mature_coin(chain: &Chain) -> Option<OutPoint> {
    coinbase_coin(chain, true)
}

/// An unspent mining reward too young to spend: one near the tip.
fn immature_coin(chain: &Chain) -> Option<OutPoint> {
    coinbase_coin(chain, false)
}

fn coinbase_coin(chain: &Chain, mature: bool) -> Option<OutPoint> {
    let next = chain.height() + 1;
    let maturity = chain.rules().coinbase_maturity;
    let heights: Vec<u64> = (1..=chain.height()).collect();
    let ordered: Vec<u64> = if mature { heights } else { heights.into_iter().rev().collect() };
    for height in ordered {
        let id = chain.id_at(height)?;
        let block = chain.block(&id)?;
        let coinbase = block.txs.first()?;
        let point = OutPoint { txid: coinbase.id(), index: 0 };
        let is_mature = next >= height.saturating_add(maturity);
        if chain.is_unspent(&point) && is_mature == mature {
            return Some(point);
        }
    }
    None
}

// --- small helpers ------------------------------------------------------------------------------

fn now_ms() -> u64 {
    crate::core::unix_ms()
}

/// A fresh key pair with no coins, used to address a block's reward and to forge signatures. Its
/// secret is fixed so the exploiter is repeatable; it owns nothing on any chain.
fn throwaway_key() -> ([u8; 32], [u8; 33]) {
    let secret = [0x5bu8; 32];
    let public = public_key(&secret).unwrap_or([0u8; 33]);
    (secret, public)
}

/// How a [`ChainError`] prints, so a verdict string can be matched against [`Trick::expected`].
fn expected_name(error: ChainError) -> &'static str {
    match error {
        ChainError::BadCoinbase => "BadCoinbase",
        ChainError::DuplicateInput => "DuplicateInput",
        ChainError::MissingInput => "MissingInput",
        ChainError::BadLock => "BadLock",
        ChainError::ImmatureCoinbase => "ImmatureCoinbase",
        ChainError::BadTime => "BadTime",
        ChainError::BadSeal => "BadSeal",
        ChainError::TooLarge => "TooLarge",
        ChainError::AlreadyKnown => "AlreadyKnown",
        ChainError::BadDifficulty => "BadDifficulty",
        ChainError::BadMerkleRoot => "BadMerkleRoot",
        ChainError::NoCoinbase => "NoCoinbase",
        ChainError::UnknownLanguage => "UnknownLanguage",
        ChainError::Overspend => "Overspend",
        ChainError::FeeTooLow => "FeeTooLow",
        ChainError::ReorgTooDeep => "ReorgTooDeep",
        ChainError::StoreFailed => "StoreFailed",
        ChainError::NotYetAllowed => "NotYetAllowed",
        ChainError::NotFinal => "NotFinal",
        ChainError::NeedsNewVersion => "NeedsNewVersion",
    }
}
