//! The wallet's keys, payments and locked side, on a small chain of its own.
use super::*;
use alloc::boxed::Box;
use olc_core::{Block, Chain, ChainError, CoinConfig, Consensus, Plugins};
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

fn seal(chain: &dyn Coins, mut block: Block) -> Block {
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

/// A locked wallet finds its coins and receives, and every way of signing says it is locked.
#[test]
fn a_locked_wallet_counts_its_coins_and_signs_nothing() {
    let (alice, bob) = wallets();
    let mut chain = chain(config());
    for _ in 0..3 {
        mine(&mut chain, &alice.address(), &[]);
    }
    let locked = alice.locked();
    assert!(!locked.is_open() && !locked.holds_secrets());
    assert_eq!(locked.balance(&chain), alice.balance(&chain));
    assert_eq!(locked.address(), alice.address());
    assert_eq!(locked.export_secret(0), None);
    assert_eq!(locked.pay(&chain, bob.address(), 1, 100, &[]), Err(WalletError::Locked));
    assert_eq!(locked.sign_message(&alice.address(), "hello"), None);
    let publics: Vec<PublicKey> = alice.keys().map(|(_, public)| public).collect();
    let watching = Wallet::from_publics(&publics, 0).unwrap();
    assert_eq!(watching.address(), alice.address());
    assert!(alice.pay(&chain, bob.address(), 1, 100, &[]).is_ok());
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
