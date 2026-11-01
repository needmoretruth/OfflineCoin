//! A wallet is a set of secret keys. It reads the chain to find its coins and signs payments, and
//! one key is the one that receives. Nothing here ever prints a secret key.
#![no_std]
extern crate alloc;

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use olc_core::{Address, Chain, OutPoint, Tx, TxIn, TxOut};
use olc_crypto::{address_of, public_key, sign, PublicKey};
use olc_lang::{alpha, ALPHA};

/// A key with this many coins ready gets them merged into one.
pub const MERGE_FROM: usize = 16;
/// Coins that go into one merge. A board checks about ten signatures a second.
const MERGE_AT_ONCE: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WalletError {
    BadKey,
    NotEnoughCoins,
    ZeroAmount,
    NoSuchKey,
    /// The node already holds this key.
    KnownKey,
}

struct Key {
    secret: [u8; 32],
    public: PublicKey,
    address: Address,
}

impl Key {
    fn new(secret: [u8; 32]) -> Result<Self, WalletError> {
        let public = public_key(&secret).ok_or(WalletError::BadKey)?;
        let address = address_of(&public);
        Ok(Self {
            secret,
            public,
            address,
        })
    }
}

pub struct Wallet {
    keys: Vec<Key>,
    receiving: usize,
}

impl Wallet {
    pub fn from_secret(secret: [u8; 32]) -> Result<Self, WalletError> {
        Ok(Self {
            keys: vec![Key::new(secret)?],
            receiving: 0,
        })
    }

    /// A wallet of several keys. `receiving` picks the one that receives.
    pub fn from_secrets(secrets: &[[u8; 32]], receiving: usize) -> Result<Self, WalletError> {
        let mut keys = Vec::with_capacity(secrets.len());
        for secret in secrets {
            keys.push(Key::new(*secret)?);
        }
        if receiving >= keys.len() {
            return Err(WalletError::NoSuchKey);
        }
        Ok(Self { keys, receiving })
    }

    /// Adds a key and returns its number.
    pub fn add_secret(&mut self, secret: [u8; 32]) -> Result<usize, WalletError> {
        let key = Key::new(secret)?;
        if self.keys.iter().any(|known| known.address == key.address) {
            return Err(WalletError::KnownKey);
        }
        self.keys.push(key);
        Ok(self.keys.len() - 1)
    }

    /// The address and public key of every key, in order.
    pub fn keys(&self) -> impl Iterator<Item = (Address, PublicKey)> + '_ {
        self.keys.iter().map(|key| (key.address, key.public))
    }

    pub fn key_count(&self) -> usize {
        self.keys.len()
    }

    pub fn receiving(&self) -> usize {
        self.receiving
    }

    pub fn set_receiving(&mut self, index: usize) -> Result<(), WalletError> {
        if index >= self.keys.len() {
            return Err(WalletError::NoSuchKey);
        }
        self.receiving = index;
        Ok(())
    }

    /// The secret key with this number, to back up. Callers must keep it off every network.
    pub fn export_secret(&self, index: usize) -> Option<[u8; 32]> {
        self.keys.get(index).map(|key| key.secret)
    }

    /// The address that receives.
    pub fn address(&self) -> Address {
        self.keys[self.receiving].address
    }

    pub fn owns(&self, lock: &[u8]) -> bool {
        self.keys.iter().any(|key| key.address.as_slice() == lock)
    }

    pub fn balance(&self, chain: &Chain) -> u64 {
        self.keys.iter().map(|key| chain.balance(&key.address)).sum()
    }

    /// Picks mature coins, pays `to` and returns the change to this wallet.
    /// `reserved` lists outputs already used by transactions waiting in the mempool.
    pub fn pay(
        &self,
        chain: &Chain,
        to: Address,
        amount: u64,
        fee: u64,
        reserved: &[OutPoint],
    ) -> Result<Tx, WalletError> {
        if amount == 0 {
            return Err(WalletError::ZeroAmount);
        }
        self.spend(chain, &[(to, amount)], &[], fee, reserved)
    }

    /// One transaction paying many addresses.
    pub fn pay_many(
        &self,
        chain: &Chain,
        payments: &[(Address, u64)],
        fee: u64,
        reserved: &[OutPoint],
    ) -> Result<Tx, WalletError> {
        if payments.is_empty() || payments.iter().any(|(_, amount)| *amount == 0) {
            return Err(WalletError::ZeroAmount);
        }
        self.spend(chain, payments, &[], fee, reserved)
    }

    /// A transaction that carries `data` and pays nobody.
    pub fn note(
        &self,
        chain: &Chain,
        data: &[u8],
        fee: u64,
        reserved: &[OutPoint],
    ) -> Result<Tx, WalletError> {
        self.spend(chain, &[], data, fee, reserved)
    }

    fn spend(
        &self,
        chain: &Chain,
        payments: &[(Address, u64)],
        data: &[u8],
        fee: u64,
        reserved: &[OutPoint],
    ) -> Result<Tx, WalletError> {
        let next = chain.height() + 1;
        let total = payments.iter().try_fold(fee, |sum, (_, amount)| sum.checked_add(*amount));
        let needed = total.ok_or(WalletError::NotEnoughCoins)?;
        let mut chosen = Vec::new();
        let mut gathered = 0u64;
        'keys: for (number, key) in self.keys.iter().enumerate() {
            for (point, entry) in chain.unspent_of(&key.address) {
                if !chosen.is_empty() && gathered >= needed {
                    break 'keys;
                }
                if chain.is_mature(&entry, next) && !reserved.contains(&point) {
                    gathered += entry.output.value;
                    chosen.push((number, point));
                }
            }
        }
        if gathered < needed || chosen.is_empty() {
            return Err(WalletError::NotEnoughCoins);
        }
        let plain = |(to, value): &(Address, u64)| TxOut {
            value: *value,
            language: ALPHA,
            lock: to.to_vec(),
        };
        let mut outputs: Vec<TxOut> = payments.iter().map(plain).collect();
        if !data.is_empty() {
            outputs.push(TxOut::data(data.to_vec()));
        }
        let change = gathered - needed;
        if change > 0 {
            outputs.push(plain(&(self.address(), change)));
        }
        self.signed(&chosen, outputs)
    }

    /// Merges small coins of one key into a single coin, so a miner's many rewards do not stay in
    /// every node's unspent set. An ordinary transaction. `None` when no key has enough of them.
    pub fn merge(&self, chain: &Chain, fee: u64, reserved: &[OutPoint]) -> Option<Tx> {
        let next = chain.height() + 1;
        for (number, key) in self.keys.iter().enumerate() {
            let mut coins: Vec<(u64, OutPoint)> = chain
                .unspent_of(&key.address)
                .into_iter()
                .filter(|(point, entry)| {
                    chain.is_mature(entry, next) && !reserved.contains(point)
                })
                .map(|(point, entry)| (entry.output.value, point))
                .collect();
            if coins.len() < MERGE_FROM {
                continue;
            }
            coins.sort_unstable_by_key(|(value, _)| *value);
            coins.truncate(MERGE_AT_ONCE);
            let total: u64 = coins.iter().map(|(value, _)| *value).sum();
            let value = total.checked_sub(fee).filter(|value| *value > 0)?;
            let chosen: Vec<(usize, OutPoint)> =
                coins.iter().map(|(_, point)| (number, *point)).collect();
            let output = TxOut {
                value,
                language: ALPHA,
                lock: key.address.to_vec(),
            };
            return self.signed(&chosen, vec![output]).ok();
        }
        None
    }

    fn signed(
        &self,
        coins: &[(usize, OutPoint)],
        outputs: Vec<TxOut>,
    ) -> Result<Tx, WalletError> {
        let unlocked = |(_, previous): &(usize, OutPoint)| TxIn {
            previous: *previous,
            unlock: Vec::new(),
        };
        let mut tx = Tx {
            version: 1,
            inputs: coins.iter().map(unlocked).collect(),
            outputs,
            coinbase_height: None,
        };
        for (index, (number, _)) in coins.iter().enumerate() {
            let key = &self.keys[*number];
            let digest = tx.signature_hash(index, &key.address);
            let signature = sign(&key.secret, &digest).ok_or(WalletError::BadKey)?;
            tx.inputs[index].unlock = alpha::unlock_data(&key.public, &signature);
        }
        Ok(tx)
    }
}

pub fn format_address(address: &Address) -> String {
    let mut text = String::with_capacity(43);
    text.push_str("olc");
    for byte in address {
        let _ = core::fmt::Write::write_fmt(&mut text, format_args!("{byte:02x}"));
    }
    text
}

pub fn parse_address(text: &str) -> Option<Address> {
    let digits = text.strip_prefix("olc").unwrap_or(text);
    if digits.len() != 40 || !digits.is_ascii() {
        return None;
    }
    let mut address = [0u8; 20];
    for (index, byte) in address.iter_mut().enumerate() {
        *byte = u8::from_str_radix(digits.get(index * 2..index * 2 + 2)?, 16).ok()?;
    }
    Some(address)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::boxed::Box;
    use olc_core::{Block, ChainError, CoinConfig, Consensus, Plugins};
    use olc_pow::ProofOfWork;

    fn config() -> CoinConfig {
        let mut config = CoinConfig::olc();
        config.initial_difficulty = 8;
        config.rules.coinbase_maturity = 2;
        config.rules.retarget_interval = 0;
        config
    }

    fn chain(config: CoinConfig) -> Chain {
        let plugins = Plugins {
            consensus: Box::new(ProofOfWork),
            languages: olc_lang::all(),
        };
        Chain::new(config, plugins)
    }

    fn seal(chain: &Chain, mut block: Block) -> Block {
        assert!(ProofOfWork.try_seal(&mut block.header, 1_000_000));
        let _ = chain;
        block
    }

    fn mine(chain: &mut Chain, miner: &Address, txs: &[Tx]) -> Block {
        let time = chain.tip_header().time_ms + 1_000;
        let block = seal(chain, chain.block_template(miner, txs, time));
        chain.add_block(block.clone()).unwrap();
        block
    }

    fn wallets() -> (Wallet, Wallet) {
        (
            Wallet::from_secret([1; 32]).unwrap(),
            Wallet::from_secret([2; 32]).unwrap(),
        )
    }

    #[test]
    fn mine_then_pay_then_spend_change() {
        let (alice, bob) = wallets();
        let mut chain = chain(config());
        for _ in 0..3 {
            mine(&mut chain, &alice.address(), &[]);
        }
        let reward = chain.rules().reward_at(1);
        assert_eq!(alice.balance(&chain), reward * 2);

        let tx = alice
            .pay(&chain, bob.address(), reward / 2, 100, &[])
            .unwrap();
        assert_eq!(chain.check_tx(&tx, chain.height() + 1), Ok(100));
        mine(&mut chain, &bob.address(), &[tx]);
        assert_eq!(bob.balance(&chain), reward / 2);
    }

    #[test]
    fn immature_coinbase_is_not_spendable() {
        let (alice, bob) = wallets();
        let mut chain = chain(config());
        mine(&mut chain, &alice.address(), &[]);
        assert_eq!(alice.balance(&chain), 0);
        let error = alice.pay(&chain, bob.address(), 1, 0, &[]);
        assert_eq!(error, Err(WalletError::NotEnoughCoins));
    }

    #[test]
    fn stolen_coins_are_rejected() {
        let (alice, bob) = wallets();
        let mut chain = chain(config());
        for _ in 0..3 {
            mine(&mut chain, &alice.address(), &[]);
        }
        let mut tx = alice.pay(&chain, bob.address(), 1_000, 0, &[]).unwrap();
        // Bob redirects Alice's payment to himself.
        tx.outputs[0].value += 1;
        assert!(matches!(chain.check_tx(&tx, 4), Err(ChainError::BadLock)));
    }

    #[test]
    fn the_same_coin_twice_in_one_block_is_rejected() {
        let (alice, bob) = wallets();
        let mut main = chain(config());
        for _ in 0..3 {
            mine(&mut main, &alice.address(), &[]);
        }
        let tx = alice.pay(&main, bob.address(), 1_000, 0, &[]).unwrap();
        let block = main.block_template(&alice.address(), &[tx.clone(), tx], 2_000_000_000_000);
        // The template keeps the first copy and drops the second.
        assert_eq!(block.txs.len(), 2);
    }

    #[test]
    fn the_heavier_branch_wins_and_moves_coins_back() {
        let (alice, bob) = wallets();
        let mut node_a = chain(config());
        let mut node_b = chain(config());
        let shared = mine(&mut node_a, &alice.address(), &[]);
        node_b.add_block(shared).unwrap();

        let a_block = mine(&mut node_a, &alice.address(), &[]);
        let first = mine(&mut node_b, &bob.address(), &[]);
        let second = mine(&mut node_b, &bob.address(), &[]);

        assert_eq!(
            node_a.add_block(first).unwrap(),
            olc_core::AddOutcome::SideBranch
        );
        let outcome = node_a.add_block(second).unwrap();
        assert_eq!(outcome, olc_core::AddOutcome::Reorganized { replaced: 1 });
        assert_eq!(node_a.tip(), node_b.tip());
        assert_eq!(
            node_a.best_chain_from(2)[0].id(),
            node_b.best_chain_from(2)[0].id()
        );
        assert!(node_a.block(&a_block.id()).is_some());
    }

    #[test]
    fn changing_rules_forks_the_same_coin() {
        let (alice, _) = wallets();
        let mut old_rules = chain(config());
        let mut changed = config();
        changed.rules.reward *= 2;
        let mut new_rules = chain(changed);
        assert_eq!(old_rules.genesis_id(), new_rules.genesis_id());

        let block = mine(&mut new_rules, &alice.address(), &[]);
        assert_eq!(old_rules.add_block(block), Err(ChainError::BadCoinbase));
    }

    #[test]
    fn address_text_round_trip() {
        let (alice, _) = wallets();
        assert_eq!(
            parse_address(&format_address(&alice.address())),
            Some(alice.address())
        );
    }
}
