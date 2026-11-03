//! Version 2 transactions on real blocks, beside version 1 ones as SegWit sits beside legacy in
//! Bitcoin. A version 2 signature names the coin, so a payment cannot be played again on a fork;
//! its id leaves out the unlock words, so a child signed first stays good; it carries a lock time
//! and sequences, so a lock can wait for a height or for the coin's age. Each test also runs the
//! version 1 form, which is how every build before judged, and shows what that let through.
use olc_core::chain::ChainError;
use olc_core::coin::{Fork, NEVER};
use olc_core::types::{merkle_root, version_for};
use olc_core::{Address, Block, Chain, CoinConfig, Coins, Consensus, Hash, OutPoint, Plugins, Tx, TxIn, TxOut, Unspent};
use olc_core::SEQUENCE_FINAL;
use olc_lang::words::{age_lock, lock_from_text_in, unlock_for_in, Dialect};
use olc_pow::ProofOfWork;
use olc_wallet::locks::LockSpend;
use olc_wallet::Wallet;

const UPGRADE_AT: u64 = 4;
const FORK_AT: u64 = 8;
const FEE: u64 = 1_000;
/// The words `1 DROP`, which change an unlock's bytes and nothing it proves.
const PADDING: [u8; 2] = [0x51, 0x75];

fn coin(upgrade_from: u64) -> CoinConfig {
    let mut coin = CoinConfig::olc();
    coin.initial_difficulty = 8;
    coin.rules.coinbase_maturity = 1;
    coin.rules.retarget_interval = 0;
    coin.upgrade_from = upgrade_from;
    coin
}

fn chain(config: CoinConfig) -> Chain {
    let plugins = Plugins { consensus: Box::new(ProofOfWork), languages: olc_lang::all() };
    Chain::new(config, plugins)
}

fn wallet(seed: u8) -> Wallet {
    Wallet::from_secret([seed; 32]).unwrap()
}

/// A chain as a wallet set to sign version 1 sees it: no signature names the coin.
struct VersionOne<'a>(&'a Chain);

impl Coins for VersionOne<'_> {
    fn height(&self) -> u64 {
        self.0.height()
    }

    fn unspent_of(&self, address: &Address) -> Vec<(OutPoint, Unspent)> {
        self.0.unspent_of(address)
    }

    fn is_mature(&self, entry: &Unspent, spend_height: u64) -> bool {
        self.0.is_mature(entry, spend_height)
    }

    fn signing_id(&self, _: u64) -> Option<Hash> {
        None
    }
}

/// The next block with exactly `txs`, whether or not the chain would choose them.
fn block(chain: &Chain, miner: &Wallet, txs: &[Tx]) -> Block {
    let time = chain.tip_header().time_ms + 1_000;
    let mut block = chain.block_template(&miner.address(), &[], time);
    block.txs.extend_from_slice(txs);
    block.header.merkle_root = merkle_root(&block.txs);
    assert!(ProofOfWork.try_seal(&mut block.header, 1_000_000));
    block
}

fn mine(chain: &mut Chain, miner: &Wallet, txs: &[Tx]) -> Result<(), ChainError> {
    let next = block(chain, miner, txs);
    chain.add_block(next).map(drop)
}

fn mine_to(chain: &mut Chain, miner: &Wallet, height: u64) {
    while chain.height() < height {
        mine(chain, miner, &[]).unwrap();
    }
}

#[test]
fn a_version_one_and_a_version_two_payment_share_a_block_after_the_upgrade() {
    let mut chain = chain(coin(UPGRADE_AT));
    let (old, new, to) = (wallet(1), wallet(2), wallet(3).address());
    mine_to(&mut chain, &old, UPGRADE_AT);
    mine_to(&mut chain, &new, UPGRADE_AT + 3);
    let satoshi = old.pay(&VersionOne(&chain), to, 1_000_000, FEE, &[]).unwrap();
    let today = new.pay(&chain, to, 1_000_000, FEE, &[]).unwrap();
    assert_eq!((satoshi.version, today.version), (1, 2), "a wallet writes version 2 unless told");
    assert_eq!(satoshi.id(), satoshi.whole_id(), "version 1 keeps its old name");
    assert_eq!(mine(&mut chain, &old, &[satoshi, today]), Ok(()));
}

/// Two chains on one history that part at [`FORK_AT`], with `payer`'s coins on both.
fn fork() -> (Chain, Chain, Wallet) {
    let old_coin = coin(UPGRADE_AT);
    let mut new_coin = old_coin.clone();
    let mut rules = old_coin.rules_at(FORK_AT).clone();
    rules.reward *= 2;
    new_coin.forks.push(Fork { from_height: FORK_AT, rules });
    let (mut old, mut new) = (chain(old_coin), chain(new_coin));
    let payer = wallet(1);
    mine_to(&mut old, &payer, FORK_AT - 2);
    for height in 1..FORK_AT - 1 {
        let id = old.id_at(height).unwrap();
        new.add_block(old.block(&id).unwrap()).unwrap();
    }
    (old, new, payer)
}

#[test]
fn below_the_fork_one_payment_goes_through_on_both_sides() {
    let (mut old, mut new, payer) = fork();
    let paid = payer.pay(&VersionOne(&old), wallet(3).address(), 1_000_000, FEE, &[]).unwrap();
    assert_eq!(mine(&mut old, &payer, std::slice::from_ref(&paid)), Ok(()));
    assert_eq!(mine(&mut new, &payer, &[paid]), Ok(()), "the history both sides share");
}

#[test]
fn from_the_fork_no_payment_goes_from_one_side_to_the_other() {
    let (mut old, mut new, payer) = fork();
    let miner = wallet(2);
    mine_to(&mut old, &miner, FORK_AT);
    mine_to(&mut new, &miner, FORK_AT);
    assert_ne!(old.tip(), new.tip(), "the two sides part at the fork");

    let to = wallet(3).address();
    let old_one = payer.pay(&VersionOne(&old), to, 1_000_000, FEE, &[]).unwrap();
    let old_two = payer.pay(&old, to, 1_000_000, FEE, &[]).unwrap();
    let new_one = payer.pay(&VersionOne(&new), to, 1_000_000, FEE, &[]).unwrap();
    let new_two = payer.pay(&new, to, 1_000_000, FEE, &[]).unwrap();
    assert_eq!(mine(&mut new, &miner, std::slice::from_ref(&old_one)), Err(ChainError::NeedsNewVersion));
    assert_eq!(mine(&mut new, &miner, &[new_one]), Err(ChainError::NeedsNewVersion), "the fork's own too");
    assert_eq!(mine(&mut new, &miner, &[old_two]), Err(ChainError::BadLock));
    assert_eq!(mine(&mut old, &miner, std::slice::from_ref(&new_two)), Err(ChainError::BadLock));
    assert_eq!(mine(&mut new, &miner, &[new_two]), Ok(()));
    assert_eq!(mine(&mut old, &miner, &[old_one]), Ok(()), "the coin that did not fork keeps version 1");
}

/// A coin under the words `DUP HASH <address> EQUALVERIFY CHECKSIG`, whose unlock can be
/// written in more than one way, as in Bitcoin before BIP 141.
fn locked_coin(chain: &mut Chain, owner: &Wallet, dialect: Dialect, words: &str) -> (OutPoint, TxOut) {
    let (lock, _) = lock_from_text_in(dialect, words).unwrap();
    let paid = owner.pay_lock_in(chain, dialect.language(), &lock, 5_000_000, FEE, &[]).unwrap();
    let index = paid.outputs.iter().position(|output| output.lock == lock).unwrap();
    let output = paid.outputs[index].clone();
    mine(chain, owner, std::slice::from_ref(&paid)).unwrap();
    (OutPoint { txid: paid.id(), index: index as u32 }, output)
}

fn hex_of(owner: &Wallet) -> String {
    owner.address().iter().map(|byte| format!("{byte:02x}")).collect()
}

fn address_words(owner: &Wallet) -> String {
    format!("DUP HASH 0x{} EQUALVERIFY CHECKSIG", hex_of(owner))
}

/// What opens [`address_words`]: a signature, then the key.
fn address_unlock(owner: &Wallet) -> String {
    format!("sig:{0} key:{0}", hex_of(owner))
}

/// Signs a parent spending a locked coin and a child spending the parent, both before either is
/// mined, then mines the parent with its unlock written another way, then the child.
fn child_after_new_unlock_words(version_one: bool) -> Result<(), ChainError> {
    let mut chain = chain(coin(UPGRADE_AT));
    let owner = wallet(1);
    mine_to(&mut chain, &owner, UPGRADE_AT + 2);
    let (previous, output) = locked_coin(&mut chain, &owner, Dialect::Beta, &address_words(&owner));
    let next = chain.height() + 1;
    let signing = chain.signing_id(next).filter(|_| !version_one);
    let unlock = address_unlock(&owner);
    let mut spend = LockSpend::new(previous, &output, owner.address(), FEE, &unlock, signing).unwrap();
    spend.fill(&owner);
    let parent = spend.finish().unwrap();

    let child_signing = chain.signing_id(next + 1);
    let mut child = Tx {
        version: version_for(child_signing.as_ref()),
        inputs: vec![TxIn { previous: OutPoint { txid: parent.id(), index: 0 }, unlock: Vec::new(), sequence: SEQUENCE_FINAL }],
        outputs: vec![TxOut { value: parent.outputs[0].value - FEE, language: 0, lock: wallet(3).address().to_vec() }],
        coinbase_height: None,
        lock_time: 0,
    };
    owner.sign_input(&mut child, &[owner.address()], 0, child_signing.as_ref()).unwrap();

    let mut reworded = parent.clone();
    let unlock = &mut reworded.inputs[0].unlock;
    unlock.splice(0..0, PADDING);
    mine(&mut chain, &owner, &[reworded]).unwrap();
    mine(&mut chain, &owner, &[child])
}

#[test]
fn a_child_signed_before_its_parent_is_mined_stays_good_when_the_parent_is_reworded() {
    let before = child_after_new_unlock_words(true);
    assert_eq!(before, Err(ChainError::MissingInput), "a version 1 parent's id moved with its unlock");
    assert_eq!(child_after_new_unlock_words(false), Ok(()));
}

#[test]
fn a_block_still_answers_for_every_unlock_byte() {
    let mut chain = chain(coin(UPGRADE_AT));
    let owner = wallet(1);
    mine_to(&mut chain, &owner, UPGRADE_AT + 2);
    let (previous, output) = locked_coin(&mut chain, &owner, Dialect::Beta, &address_words(&owner));
    let signing = chain.signing_id(chain.height() + 1);
    let unlock = address_unlock(&owner);
    let mut spend = LockSpend::new(previous, &output, owner.address(), FEE, &unlock, signing).unwrap();
    spend.fill(&owner);
    let tx = spend.finish().unwrap();
    let mut swapped = block(&chain, &owner, &[tx]);
    swapped.txs[1].inputs[0].unlock.splice(0..0, PADDING);
    assert_eq!(chain.add_block(swapped), Err(ChainError::BadMerkleRoot), "the root covers the unlock");
}

#[test]
fn the_new_transaction_form_waits_for_the_upgrade() {
    let mut chain = chain(coin(UPGRADE_AT + 10));
    let owner = wallet(1);
    mine_to(&mut chain, &owner, UPGRADE_AT);
    let mut tx = owner.pay(&chain, wallet(3).address(), 1_000_000, FEE, &[]).unwrap();
    assert_eq!(tx.version, 1, "a wallet writes the old form before the upgrade");
    tx.version = 2;
    assert_eq!(mine(&mut chain, &owner, &[tx]), Err(ChainError::NotYetAllowed));
}

#[test]
fn a_lock_time_holds_a_payment_until_the_block_after_it() {
    let mut chain = chain(coin(UPGRADE_AT));
    let owner = wallet(1);
    mine_to(&mut chain, &owner, UPGRADE_AT + 2);
    let until = chain.height() + 2;
    let mut tx = owner.pay(&chain, wallet(3).address(), 1_000_000, FEE, &[]).unwrap();
    tx.lock_time = u32::try_from(until).unwrap();
    tx.inputs[0].sequence = SEQUENCE_FINAL - 1;
    let signing = chain.signing_id(until);
    let owners = vec![owner.address(); tx.inputs.len()];
    for index in 0..tx.inputs.len() {
        owner.sign_input(&mut tx, &owners, index, signing.as_ref()).unwrap();
    }
    mine(&mut chain, &owner, &[]).unwrap();
    assert_eq!(mine(&mut chain, &owner, std::slice::from_ref(&tx)), Err(ChainError::NotFinal), "at its own height");
    mine(&mut chain, &owner, &[]).unwrap();
    assert_eq!(mine(&mut chain, &owner, &[tx]), Ok(()));
}

/// Locks a coin in gamma for `blocks` blocks of age and spends it one block early, then when due.
fn age_lock_spends(upgrade_from: u64, blocks: u64) -> (Result<(), ChainError>, Result<(), ChainError>) {
    let mut chain = chain(coin(upgrade_from));
    let owner = wallet(1);
    mine_to(&mut chain, &owner, UPGRADE_AT + 2);
    let words = age_lock(blocks, &owner.address());
    let (previous, output) = locked_coin(&mut chain, &owner, Dialect::Gamma, &words);
    let held_since = chain.height();
    let spend_at = |chain: &Chain| {
        let signing = chain.signing_id(chain.height() + 1);
        let unlock = unlock_for_in(Dialect::Gamma, &output.lock);
        let mut spend = LockSpend::new(previous, &output, owner.address(), FEE, &unlock, signing).unwrap();
        spend.fill(&owner);
        spend.finish().unwrap()
    };
    mine_to(&mut chain, &owner, held_since + blocks - 2);
    let early_spend = spend_at(&chain);
    let early = mine(&mut chain, &owner, &[early_spend]);
    if early.is_err() {
        mine(&mut chain, &owner, &[]).unwrap();
    }
    let due_spend = spend_at(&chain);
    let due = mine(&mut chain, &owner, &[due_spend]);
    (early, due)
}

#[test]
fn checksequenceverify_opens_when_due_and_not_a_block_before() {
    assert_eq!(age_lock_spends(UPGRADE_AT, 3), (Err(ChainError::NotFinal), Ok(())));
    let (_, due) = age_lock_spends(NEVER, 3);
    assert_eq!(due, Err(ChainError::BadLock), "a coin without version 2 cannot spend gamma");
}
