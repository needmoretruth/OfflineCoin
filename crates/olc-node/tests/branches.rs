//! What a node does when a heavier branch replaces the blocks it had.
mod common;

use common::{deliver, engine, mine, silence};
use olc_core::Hash;
use olc_node::Engine;

const AMOUNT: u64 = 1_000;

fn height_of(node: &Engine, id: &Hash) -> Option<u64> {
    node.chain().find_tx(id)
}

#[test]
fn a_heavier_branch_replaces_a_confirmed_payment() {
    let mut nodes = [engine(1), engine(2)];
    let (payer_address, payee_address) = (nodes[0].address(), nodes[1].address());
    for block in 0..3 {
        mine(&mut nodes[0], block * 1_000);
        deliver(&mut nodes);
    }
    let start = nodes[1].status().balance;

    let payment = nodes[0].pay(payee_address, AMOUNT, 0).unwrap();
    deliver(&mut nodes);
    assert!(nodes[1].in_pool(&payment));

    // The payer leaves the network and forgets the payment, so the same coins are free again.
    nodes[0].clear_pool();
    assert_eq!(nodes[0].status().pool, 0);

    mine(&mut nodes[1], 10_000);
    mine(&mut nodes[1], 11_000);
    assert_eq!(height_of(&nodes[1], &payment), Some(4));
    assert!(nodes[1].status().balance >= start + AMOUNT);

    let spend = nodes[0].pay(payer_address, AMOUNT, 0).unwrap();
    assert_ne!(spend, payment);
    for block in 0..3 {
        mine(&mut nodes[0], 20_000 + block * 1_000);
    }
    silence(&mut nodes[0]);
    silence(&mut nodes[1]);

    nodes[0].tick(40_000);
    deliver(&mut nodes);

    assert_eq!(nodes[1].status().tip, nodes[0].status().tip);
    assert_eq!(height_of(&nodes[1], &payment), None);
    assert_eq!(height_of(&nodes[0], &payment), None);
    assert_eq!(height_of(&nodes[1], &spend), Some(4));
    assert!(!nodes[1].in_pool(&payment));
    assert_eq!(nodes[1].status().balance, start);
    assert_eq!(
        nodes[1].chain().balance(&payer_address),
        nodes[0].status().balance
    );
}

#[test]
fn the_shown_balance_follows_the_tip() {
    let mut nodes = [engine(1), engine(2)];
    let mut before = nodes[0].status().balance;
    for block in 0..3 {
        mine(&mut nodes[0], block * 1_000);
        let now = nodes[0].status().balance;
        assert_eq!(now, nodes[0].chain().balance(&nodes[0].address()));
        assert!(now >= before);
        before = now;
    }
    deliver(&mut nodes);
    assert_eq!(
        nodes[1].status().balance,
        nodes[1].chain().balance(&nodes[1].address())
    );
}

#[test]
fn a_stale_template_keeps_its_payment_and_takes_the_new_time() {
    let mut nodes = [engine(1), engine(2)];
    for block in 0..3 {
        mine(&mut nodes[0], block * 1_000);
    }
    let tip_time = nodes[0].chain().tip_header().time_ms;
    nodes[0].tick(1_000);
    let to = nodes[1].address();
    let payment = nodes[0].pay(to, AMOUNT, 0).unwrap();
    assert!(!nodes[0].mine(0, 1_000));
    mine(&mut nodes[0], 11_000);
    assert_eq!(height_of(&nodes[0], &payment), Some(4));
    assert_eq!(nodes[0].chain().tip_header().time_ms, tip_time + 10_000);
}
