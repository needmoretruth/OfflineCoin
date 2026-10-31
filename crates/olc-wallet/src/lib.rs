//! A wallet is one secret key. It reads the chain to find its coins and signs payments.
#![no_std]
extern crate alloc;

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use olc_core::{Address, Chain, OutPoint, Tx, TxIn, TxOut};
use olc_crypto::{address_of, public_key, sign, PublicKey};
use olc_lang::{alpha, ALPHA};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WalletError {
    BadKey,
    NotEnoughCoins,
    ZeroAmount,
}

pub struct Wallet {
    secret: [u8; 32],
    public: PublicKey,
    address: Address,
}

impl Wallet {
    pub fn from_secret(secret: [u8; 32]) -> Result<Self, WalletError> {
        let public = public_key(&secret).ok_or(WalletError::BadKey)?;
        Ok(Self {
            secret,
            public,
            address: address_of(&public),
        })
    }

    pub fn address(&self) -> Address {
        self.address
    }

    pub fn balance(&self, chain: &Chain) -> u64 {
        chain.balance(&self.address)
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
        let next = chain.height() + 1;
        let needed = amount.checked_add(fee).ok_or(WalletError::NotEnoughCoins)?;
        let mut inputs = Vec::new();
        let mut gathered = 0u64;
        for (point, entry) in chain.unspent_of(&self.address) {
            if gathered >= needed {
                break;
            }
            if chain.is_mature(&entry, next) && !reserved.contains(&point) {
                gathered += entry.output.value;
                inputs.push(TxIn {
                    previous: point,
                    unlock: Vec::new(),
                });
            }
        }
        if gathered < needed {
            return Err(WalletError::NotEnoughCoins);
        }
        let mut outputs = vec![TxOut {
            value: amount,
            language: ALPHA,
            lock: to.to_vec(),
        }];
        let change = gathered - needed;
        if change > 0 {
            outputs.push(TxOut {
                value: change,
                language: ALPHA,
                lock: self.address.to_vec(),
            });
        }
        let mut tx = Tx {
            version: 1,
            inputs,
            outputs,
            coinbase_height: None,
        };
        for index in 0..tx.inputs.len() {
            let digest = tx.signature_hash(index, &self.address);
            let signature = sign(&self.secret, &digest).ok_or(WalletError::BadKey)?;
            tx.inputs[index].unlock = alpha::unlock_data(&self.public, &signature);
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
    fn double_spend_in_one_block_is_rejected() {
        let (alice, bob) = wallets();
        let mut main = chain(config());
        for _ in 0..3 {
            mine(&mut main, &alice.address(), &[]);
        }
        let tx = alice.pay(&main, bob.address(), 1_000, 0, &[]).unwrap();
        let block = main.block_template(&alice.address(), &[tx.clone(), tx], 2_000_000_000_000);
        // The template keeps the first copy and drops the double spend.
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
