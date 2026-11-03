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
