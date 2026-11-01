//! One node's dealings with its neighbours.
mod common;

use common::{deliver, engine, mine};
use olc_core::{ChainError, OutPoint};
use olc_node::{Engine, Event, Message};
use olc_wallet::Wallet;

#[test]
fn a_late_node_catches_up_through_tip_announcements() {
    let mut nodes = [engine(1), engine(2)];
    for block in 0..5 {
        mine(&mut nodes[0], block * 1_000);
    }
    while nodes[0].take_outgoing().is_some() {}
    nodes[0].tick(10_000);
    deliver(&mut nodes);
    assert_eq!(nodes[1].status().tip, nodes[0].status().tip);
    assert_eq!(nodes[1].status().height, 5);
}

#[test]
fn every_new_block_is_queued_for_saving() {
    let mut node = engine(1);
    mine(&mut node, 1_000);
    mine(&mut node, 2_000);
    let first = node.take_fresh_block().unwrap();
    let second = node.take_fresh_block().unwrap();
    assert_eq!(second.header.previous, first.id());
    assert!(node.take_fresh_block().is_none());
}

#[test]
fn a_payment_crosses_the_network() {
    let mut nodes = [engine(1), engine(2)];
    for block in 0..3 {
        mine(&mut nodes[0], block * 1_000);
        deliver(&mut nodes);
    }
    let to = nodes[1].address();
    nodes[0].pay(to, 1_000, 10).unwrap();
    deliver(&mut nodes);
    assert_eq!(nodes[1].status().pool, 1);

    mine(&mut nodes[1], 9_000);
    deliver(&mut nodes);
    assert_eq!(nodes[0].status().pool, 0);
    assert_eq!(nodes[0].status().tip, nodes[1].status().tip);
    assert!(nodes[0].chain().balance(&to) >= 1_000);
}

#[test]
fn a_forged_payment_is_rejected_by_peers() {
    let mut nodes = [engine(1), engine(2)];
    for block in 0..3 {
        mine(&mut nodes[0], block * 1_000);
        deliver(&mut nodes);
    }
    let to = nodes[1].address();
    let reserved: Vec<OutPoint> = Vec::new();
    let wallet = Wallet::from_secret([1; 32]).unwrap();
    let mut forged = wallet
        .pay(nodes[0].chain(), to, 1_000, 0, &reserved)
        .unwrap();
    forged.outputs[0].value = 2_000;
    nodes[1].on_message(0, Message::Tx(forged));
    assert_eq!(nodes[1].status().pool, 0);
    assert_eq!(take_rejection(&mut nodes[1]), Some(ChainError::BadLock));
}

fn take_rejection(node: &mut Engine) -> Option<ChainError> {
    while let Some(event) = node.take_event() {
        if let Event::TxRejected { reason } = event {
            return Some(reason);
        }
    }
    None
}
