//! A forged spend of a beta lock reaches a node as any stranger's transaction would, and the node's
//! own check is what stops it: nothing here goes through the sender's wallet or its checks.
mod common;

use common::{deliver, engine_of, mine, silence, test_coin};
use olc_core::coin::COIN;
use olc_core::{OutPoint, Tx, TxIn, TxOut};
use olc_crypto::hash;
use olc_lang::words::{assemble, hash_lock, parse_words};
use olc_lang::ALPHA;
use olc_node::Engine;

const SECRET: &str = "a long way home";

fn nodes() -> Vec<Engine> {
    vec![engine_of(1, test_coin()), engine_of(2, test_coin())]
}

/// Two blocks mined, one matured, and coins put under `words` in a third.
fn locked(nodes: &mut [Engine], words: &str) -> (OutPoint, Tx) {
    mine(&mut nodes[0], 1_000);
    mine(&mut nodes[0], 2_000);
    deliver(nodes);
    let (txid, _) = nodes[0].pay_to_lock(words, 5 * COIN).unwrap();
    deliver(nodes);
    mine(&mut nodes[0], 3_000);
    deliver(nodes);
    assert_eq!(nodes[0].chain().tip(), nodes[1].chain().tip());
    let coin = nodes[1].locked_coins().pop().unwrap();
    assert_eq!(coin.point.txid, txid);
    let out = TxOut {
        value: coin.value - nodes[1].rules().min_fee,
        language: ALPHA,
        lock: nodes[1].address().to_vec(),
    };
    let tx = Tx { version: 1, inputs: Vec::new(), outputs: vec![out], coinbase_height: None };
    (coin.point, tx)
}

fn with_unlock(mut tx: Tx, point: OutPoint, words: &str) -> Tx {
    let (unlock, _) = assemble(&parse_words(words).unwrap()).unwrap();
    tx.inputs = vec![TxIn { previous: point, unlock }];
    tx
}

#[test]
fn a_wrong_unlock_from_a_stranger_goes_nowhere() {
    let mut nodes = nodes();
    let lock = hash_lock(&hash(SECRET.as_bytes())[..20]);
    let (point, tx) = locked(&mut nodes, &lock);

    let forged = with_unlock(tx.clone(), point, "\"the wrong words\"");
    assert!(!nodes[0].submit_tx(forged.clone()));
    assert!(!nodes[1].submit_tx(forged));
    silence(&mut nodes[0]);
    silence(&mut nodes[1]);
    assert!(nodes[1].chain().coin_at(&point).is_some());

    let real = with_unlock(tx, point, &format!("\"{SECRET}\""));
    assert!(nodes[1].submit_tx(real));
    deliver(&mut nodes);
    mine(&mut nodes[0], 4_000);
    deliver(&mut nodes);
    assert!(nodes[1].chain().coin_at(&point).is_none());
    assert!(nodes[0].locked_coins().is_empty());
}

#[test]
fn a_block_that_carries_a_forged_spend_is_refused() {
    let mut nodes = nodes();
    let lock = hash_lock(&hash(SECRET.as_bytes())[..20]);
    let (point, tx) = locked(&mut nodes, &lock);
    let forged = with_unlock(tx, point, "\"the wrong words\"");
    let next = nodes[0].chain().height() + 1;
    assert!(nodes[0].chain().check_tx(&forged, next).is_err());
}
