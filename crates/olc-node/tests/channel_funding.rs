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

/// The figure a person sees as spendable and the coins a payment picks both leave out the
/// coins held for a channel, and get them back when the hold ends.
#[test]
fn held_coins_leave_the_spendable_figure_and_the_pay_path() {
    let mut node = engine(2);
    for now in 1..=4 {
        mine(&mut node, now * 1_000);
    }
    let fee = node.rules().min_fee;
    let all = node.spendable();
    let held = node.pick_funding(COIN, fee).unwrap();
    node.hold_coins(held.clone());
    let free = node.spendable();
    assert!(all - free >= COIN + fee, "the held coins left the figure: {all} then {free}");

    let somebody = [9; 20];
    assert!(node.pay(somebody, free - fee + 1, fee).is_err(), "a payment found more than the figure");
    let paid = node.pay(somebody, free - fee, fee);
    assert!(paid.is_ok(), "a payment of the whole figure: {paid:?}");
    assert!(inputs(&node).iter().all(|point| !held.contains(point)), "a held coin went into the payment");
    assert_eq!(node.spendable(), 0);

    node.hold_coins(Vec::new());
    assert_eq!(node.spendable(), all - free, "the coins come back when the hold ends");
}

/// A funding waiting for its block is sped up by a child: signing it again would change its id,
/// and the balances both sides signed spend that id.
#[test]
fn a_waiting_funding_is_sped_up_by_a_child_and_keeps_its_id() {
    let mut node = engine(3);
    for now in 1..=4 {
        mine(&mut node, now * 1_000);
    }
    let fee = node.rules().min_fee;
    let held = node.pick_funding(COIN, fee).unwrap();
    node.hold_coins(held.clone());
    let funding = node.sign_funding(&[7u8; 71], COIN, fee, &held).unwrap();
    let id = node.publish(funding).unwrap();
    let sped = node.speed_up(&id).unwrap();
    assert!(!sped.replaced, "the funding was signed again under another id");
    assert!(node.in_pool(&id), "the funding left the pool");
    assert!(node.in_pool(&sped.id));
}

/// A funding spends only coins in blocks, never change still waiting, which a faster payment in
/// its place could take away.
#[test]
fn a_funding_takes_no_change_still_waiting() {
    let mut node = engine(4);
    for now in 1..=4 {
        mine(&mut node, now * 1_000);
    }
    let fee = node.rules().min_fee;
    let all = node.spendable();
    node.pay([9; 20], all - fee - 3 * COIN, fee).unwrap();
    assert_eq!(node.spendable(), 3 * COIN, "the change counts for a payment");
    assert!(node.pick_funding(COIN, fee).is_err(), "a funding took change still waiting");
    mine(&mut node, 5_000);
    assert!(node.pick_funding(COIN, fee).is_ok(), "after the block it can");
}
