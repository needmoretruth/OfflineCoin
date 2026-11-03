//! Coins on one key moved to another key of the same wallet in one payment, as a wallet made
//! before seed words moves its coins to the words.
mod common;

use common::{deliver, engine, mine};

#[test]
fn every_coin_of_the_old_key_arrives_on_the_new_one() {
    let mut nodes = [engine(11), engine(12)];
    for block in 0..4 {
        mine(&mut nodes[0], block * 1_000);
        deliver(&mut nodes);
    }
    let old = nodes[0].address();
    let index = nodes[0].wallet_mut().add_secret([13; 32]).unwrap();
    let new = nodes[0].wallet().keys().nth(index).unwrap().0;
    let held = |node: &olc_node::Engine, address| -> u64 {
        node.coins().unspent_of(&address).iter().map(|(_, entry)| entry.output.value).sum()
    };
    let before = held(&nodes[0], old);
    assert!(before > 0);

    let (id, arrives) = nodes[0].move_coins(&[old], new).unwrap();
    deliver(&mut nodes);
    assert!(nodes[1].in_pool(&id));
    mine(&mut nodes[1], 10_000);
    deliver(&mut nodes);
    let fee = nodes[0].rules().min_fee;
    // Only the coins that were ripe go; the newest reward may still be maturing.
    assert!(arrives + fee <= before);
    assert_eq!(held(&nodes[0], new), arrives);
    assert!(nodes[0].move_coins(&[new], new).is_ok(), "the moved coins can be spent again");
}

/// A payment of this wallet still waiting for a block leaves its change, or what it moved, to
/// spend at once, as Bitcoin Core spends its own change; and the figure a person sees as
/// spendable is exactly what a payment can spend.
#[test]
fn what_a_waiting_payment_of_this_wallet_leaves_is_spendable_at_once() {
    let mut node = engine(21);
    for block in 0..4 {
        mine(&mut node, block * 1_000);
    }
    let old = node.address();
    let index = node.wallet_mut().add_secret([22; 32]).unwrap();
    let new = node.wallet().keys().nth(index).unwrap().0;
    let (id, arrives) = node.move_coins(&[old], new).unwrap();
    assert!(node.in_pool(&id), "nothing mined since: the move waits");
    assert_eq!(node.spendable(), arrives, "the coins the move spends are gone, what it moves is there");
    let fee = node.rules().min_fee;
    let somebody = [9; 20];
    let paid = node.pay(somebody, arrives / 2, fee);
    assert!(paid.is_ok(), "{paid:?}");
    assert_eq!(node.spendable(), arrives - arrives / 2 - fee, "the change of a waiting payment counts too");
}
