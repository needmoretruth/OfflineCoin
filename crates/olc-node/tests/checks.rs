//! A node's limit on signature checks in one transaction: a stranger's transaction over it stops
//! before any signature is checked, while a block that carries one stays valid, because this is
//! what a node passes on and not what the coin allows.
mod common;

use common::{deliver, engine_of, mine, test_coin};
use olc_core::coin::COIN;
use olc_core::{Block, OutPoint, Tx, TxIn, TxOut};
use olc_lang::beta::op;
use olc_lang::words::{assemble, parse_words, WordError, MOST_SIGNATURE_CHECKS};
use olc_lang::ALPHA;
use olc_node::locks::LockFailure;
use olc_node::{Engine, Event};
use olc_wallet::locks::LockError;
use olc_wallet::WalletError;

/// Costs `checks` signature checks, and the script passes whatever they find.
fn lock_of(checks: usize) -> Vec<u8> {
    let mut lock = Vec::new();
    for _ in 0..checks {
        lock.extend_from_slice(&[op::ONE, op::ONE, op::CHECK_SIG, op::DROP]);
    }
    lock.push(op::ONE);
    lock
}

fn nodes() -> Vec<Engine> {
    vec![engine_of(1, test_coin()), engine_of(2, test_coin())]
}

/// A coin under `lock` on both nodes, and a transaction that spends it to the second.
fn spend_of(nodes: &mut [Engine], lock: &[u8]) -> Tx {
    mine(&mut nodes[0], 1_000);
    mine(&mut nodes[0], 2_000);
    deliver(nodes);
    let fee = nodes[0].rules().min_fee;
    let payment = nodes[0].wallet().pay_lock(nodes[0].coins(), lock, 5 * COIN, fee, &[]).unwrap();
    let txid = payment.id();
    let index = payment.outputs.iter().position(|out| out.lock == lock).unwrap();
    nodes[0].send_signed(payment);
    deliver(nodes);
    mine(&mut nodes[0], 3_000);
    deliver(nodes);
    let previous = OutPoint { txid, index: u32::try_from(index).unwrap() };
    let out = TxOut { value: 5 * COIN - fee, language: ALPHA, lock: nodes[1].address().to_vec() };
    let input = TxIn { previous, unlock: Vec::new() };
    Tx { version: 1, inputs: vec![input], outputs: vec![out], coinbase_height: None }
}

fn events(node: &mut Engine) -> Vec<Event> {
    core::iter::from_fn(|| node.take_event()).collect()
}

/// A block on `node`'s tip holding `tx`, which its pool would never give a miner.
fn sealed_with(node: &Engine, tx: Tx, now: u64) -> Block {
    let mut block = node.chain().block_template(&node.address(), core::slice::from_ref(&tx), now);
    assert!(block.txs.contains(&tx), "the block rules take what a pool does not");
    let difficulty = block.header.difficulty;
    assert!(node.grind(&mut block.header, difficulty, u32::MAX));
    block
}

#[test]
fn a_transaction_past_the_limit_is_not_taken_passed_on_or_mined() {
    let mut nodes = nodes();
    let tx = spend_of(&mut nodes, &lock_of(MOST_SIGNATURE_CHECKS + 1));
    let id = tx.id();
    assert_eq!(nodes[1].chain().signature_checks(&tx), MOST_SIGNATURE_CHECKS + 1);
    assert!(nodes[1].chain().check_tx(&tx, nodes[1].chain().height() + 1).is_ok(), "only policy stops it");
    events(&mut nodes[1]);

    assert!(!nodes[1].submit_tx(tx));
    let told = events(&mut nodes[1]);
    assert!(told.contains(&Event::ChecksRefused { checks: MOST_SIGNATURE_CHECKS + 1 }), "{told:?}");
    assert!(!nodes[1].in_pool(&id));
    assert!(nodes[1].take_outgoing().is_none(), "nothing is passed on");
    let template = nodes[1].work_template(&nodes[1].address(), 9_000, &[]);
    assert!(template.txs.iter().all(|tx| tx.id() != id), "nothing is mined");
}

#[test]
fn a_transaction_at_the_limit_goes_through() {
    let mut nodes = nodes();
    let tx = spend_of(&mut nodes, &lock_of(MOST_SIGNATURE_CHECKS));
    let id = tx.id();
    assert!(nodes[1].submit_tx(tx));
    deliver(&mut nodes);
    assert!(nodes[0].in_pool(&id));
}

#[test]
fn a_block_past_the_limit_is_still_valid() {
    let mut nodes = nodes();
    let tx = spend_of(&mut nodes, &lock_of(MOST_SIGNATURE_CHECKS + 1));
    let block = sealed_with(&nodes[0], tx.clone(), 9_000);
    let tip = block.header.id();
    assert!(nodes[1].submit_block(block));
    assert_eq!(nodes[1].chain().tip(), tip);
    assert!(nodes[1].chain().find_tx(&tx.id()).is_some());
}

#[test]
fn a_heavy_payment_that_leaves_a_block_stays_out_of_the_pool() {
    let mut nodes = nodes();
    let tx = spend_of(&mut nodes, &lock_of(MOST_SIGNATURE_CHECKS + 1));
    let id = tx.id();
    let block = sealed_with(&nodes[0], tx, 9_000);
    assert!(nodes[1].submit_block(block));
    events(&mut nodes[1]);

    mine(&mut nodes[0], 10_000);
    mine(&mut nodes[0], 11_000);
    mine(&mut nodes[0], 12_000);
    deliver(&mut nodes);
    assert_eq!(nodes[1].chain().tip(), nodes[0].chain().tip(), "the longer branch won");
    assert!(nodes[1].chain().find_tx(&id).is_none());
    assert!(!nodes[1].in_pool(&id));
    let told = events(&mut nodes[1]);
    assert!(told.iter().any(|event| matches!(event, Event::TxDropped { id: found, .. } if *found == id)));
}

/// A spend no pool takes reaches nobody, so its owner hears that and not a hash that looks sent.
#[test]
fn the_owner_of_a_heavy_lock_hears_that_the_spend_went_nowhere() {
    let mut nodes = nodes();
    let tx = spend_of(&mut nodes, &lock_of(MOST_SIGNATURE_CHECKS + 1));
    let to = nodes[1].address();
    let spend = nodes[1].spend_lock(tx.inputs[0].previous, to, "1").unwrap();
    let failure = nodes[1].finish_lock(spend).unwrap_err();
    assert!(matches!(failure, LockFailure::Lock(LockError::Wallet(WalletError::Refused))), "{failure:?}");
}

/// A person who writes such a lock hears it at once, before any coins are under it.
#[test]
fn a_lock_that_could_never_be_spent_is_refused_in_words() {
    let words = "1 1 CHECKSIG DROP ".repeat(MOST_SIGNATURE_CHECKS + 1);
    let parsed = parse_words(&words).unwrap();
    assert_eq!(assemble(&parsed).unwrap_err(), WordError::TooManyChecks(MOST_SIGNATURE_CHECKS + 1));
    let words = "1 1 CHECKSIG DROP ".repeat(MOST_SIGNATURE_CHECKS);
    let (_, cost) = assemble(&parse_words(&words).unwrap()).unwrap();
    assert_eq!(cost.signature_checks, MOST_SIGNATURE_CHECKS);
}

/// The lock page's 2-of-2 costs two checks, as the page says.
#[test]
fn a_two_of_two_counts_as_two_checks() {
    let key = [2u8; 33];
    let words = olc_lang::words::two_of_two(&key, &key);
    let (_, cost) = assemble(&parse_words(&words).unwrap()).unwrap();
    assert_eq!(cost.signature_checks, 2);
}
