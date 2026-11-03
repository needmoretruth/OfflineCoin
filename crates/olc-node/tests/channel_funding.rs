//! Coins picked for a payment channel stay out of every other payment until the channel is
//! funded, and the funding spends exactly those coins.
mod common;

use common::{engine, mine};
use olc_core::coin::COIN;
use olc_core::OutPoint;

fn inputs(engine: &olc_node::Engine) -> Vec<OutPoint> {
    engine.pool().iter().flat_map(|tx| tx.inputs.iter().map(|input| input.previous)).collect()
}

#[test]
fn held_coins_are_not_spent_by_another_payment() {
    let mut node = engine(1);
    for now in 1..=4 {
        mine(&mut node, now * 1_000);
    }
    let fee = node.rules().min_fee;
    let held = node.pick_funding(COIN, fee).unwrap();
    assert!(!held.is_empty());
    node.hold_coins(held.clone());

    // The same pick the wallet would make first, were the coins not held.
    let elsewhere = olc_wallet::Wallet::from_secret([9; 32]).unwrap().address();
    node.pay(elsewhere, COIN, fee).unwrap();
    let spent = inputs(&node);
    assert!(!spent.is_empty());
    assert!(spent.iter().all(|point| !held.contains(point)), "a held coin went into another payment");

    // The funding takes the held coins and nothing else, while that payment still waits.
    let lock = [7u8; 71];
    let funding = node.sign_funding(&lock, COIN, fee, &held).unwrap();
    let mut from: Vec<OutPoint> = funding.inputs.iter().map(|input| input.previous).collect();
    let mut wanted = held.clone();
    from.sort();
    wanted.sort();
    assert_eq!(from, wanted);
}
