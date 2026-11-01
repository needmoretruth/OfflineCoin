//! Bitcoin's reorganisation rule: payments from blocks that left the best chain are checked again.
//! The valid ones wait to be mined again, the rest are thrown out.
mod common;

use common::{deliver, engine, engine_with_maturity, mine, silence};
use olc_core::{ChainError, Hash};
use olc_node::{Engine, Event};

const AMOUNT: u64 = 1_000;

fn take_events(node: &mut Engine) -> Vec<Event> {
    core::iter::from_fn(|| node.take_event()).collect()
}

fn dropped(events: &[Event], id: Hash) -> Option<Option<ChainError>> {
    events.iter().find_map(|event| match event {
        Event::TxDropped { id: found, reason } if *found == id => Some(*reason),
        _ => None,
    })
}

/// Three nodes share `blocks` blocks mined by the first one.
fn network(blocks: u64) -> [Engine; 3] {
    let mut nodes = [engine(1), engine(2), engine(3)];
    for block in 0..blocks {
        mine(&mut nodes[0], block * 1_000);
        deliver(&mut nodes);
    }
    nodes
}

/// The third node, cut off until now, mines a heavier chain and comes back.
fn heavier_branch_arrives(nodes: &mut [Engine; 3], blocks: u64) {
    for block in 0..blocks {
        mine(&mut nodes[2], 500_000 + block * 1_000);
    }
    deliver(nodes);
}

#[test]
fn a_payment_survives_a_plain_reorganisation_and_confirms_again() {
    let mut nodes = network(3);
    let to = nodes[1].address();
    let payment = nodes[0].pay(to, AMOUNT, 0).unwrap();
    deliver(&mut nodes[..2]);
    mine(&mut nodes[0], 100_000);
    deliver(&mut nodes[..2]);
    assert_eq!(nodes[1].chain().find_tx(&payment), Some(4));

    heavier_branch_arrives(&mut nodes, 2);

    assert_eq!(nodes[0].chain().find_tx(&payment), None);
    assert!(nodes[0].in_pool(&payment));
    assert!(nodes[2].in_pool(&payment), "the winning side hears of it");
    let events = take_events(&mut nodes[0]);
    assert!(events.contains(&Event::TxReturned { id: payment }));

    mine(&mut nodes[2], 700_000);
    deliver(&mut nodes);
    assert_eq!(nodes[0].chain().find_tx(&payment), Some(6));
    assert_eq!(nodes[0].status().pool, 0);
}

#[test]
fn a_payment_whose_coins_were_spent_again_is_dropped() {
    let mut nodes = network(3);
    let (to, own) = (nodes[1].address(), nodes[0].address());
    let payment = nodes[0].pay(to, AMOUNT, 0).unwrap();
    deliver(&mut nodes[..2]);
    // The payer forgets it and mines the same coins to itself, out of sight.
    nodes[0].clear_pool();
    let spend = nodes[0].pay(own, AMOUNT, 0).unwrap();
    silence(&mut nodes[0]);
    for block in 0..3 {
        mine(&mut nodes[0], 200_000 + block * 1_000);
    }

    mine(&mut nodes[1], 100_000);
    assert_eq!(nodes[1].chain().find_tx(&payment), Some(4));
    take_events(&mut nodes[1]);
    deliver(&mut nodes[..2]);

    assert_eq!(nodes[1].chain().find_tx(&payment), None);
    assert!(nodes[1].chain().find_tx(&spend).is_some());
    assert!(!nodes[1].in_pool(&payment));
    let events = take_events(&mut nodes[1]);
    assert_eq!(
        dropped(&events, payment),
        Some(Some(ChainError::MissingInput))
    );
}

#[test]
fn a_payment_on_both_branches_stays_confirmed_and_silent() {
    let mut nodes = network(3);
    let to = nodes[1].address();
    let payment = nodes[0].pay(to, AMOUNT, 0).unwrap();
    deliver(&mut nodes);
    mine(&mut nodes[0], 100_000);
    deliver(&mut nodes[..2]);
    take_events(&mut nodes[0]);

    heavier_branch_arrives(&mut nodes, 2);

    assert_eq!(nodes[0].chain().find_tx(&payment), Some(4));
    assert_eq!(nodes[0].status().pool, 0);
    assert!(take_events(&mut nodes[0])
        .iter()
        .all(|event| !matches!(event, Event::TxReturned { .. } | Event::TxDropped { .. })));
}

#[test]
fn a_payment_of_a_vanished_reward_is_dropped() {
    let mut nodes = network(3);
    let to = nodes[1].address();
    mine(&mut nodes[1], 100_000);
    deliver(&mut nodes[..2]);
    // The second node's only coin is the reward of a block about to leave the chain.
    let payment = nodes[1].pay(to, AMOUNT, 0).unwrap();
    deliver(&mut nodes[..2]);
    mine(&mut nodes[0], 101_000);
    deliver(&mut nodes[..2]);
    assert!(nodes[0].chain().find_tx(&payment).is_some());
    take_events(&mut nodes[0]);

    heavier_branch_arrives(&mut nodes, 3);

    let events = take_events(&mut nodes[0]);
    assert_eq!(
        dropped(&events, payment),
        Some(Some(ChainError::MissingInput))
    );
    assert_eq!(nodes[0].status().pool, 0);
}

#[test]
fn a_payment_and_the_one_spending_it_come_back_in_order() {
    let mut nodes = network(3);
    let (middle, last) = (nodes[1].address(), nodes[2].address());
    let first = nodes[0].pay(middle, AMOUNT, 0).unwrap();
    mine(&mut nodes[0], 100_000);
    deliver(&mut nodes[..2]);
    let second = nodes[1].pay(last, AMOUNT / 2, 0).unwrap();
    deliver(&mut nodes[..2]);
    mine(&mut nodes[0], 101_000);
    deliver(&mut nodes[..2]);
    assert_eq!(nodes[0].chain().find_tx(&second), Some(5));

    heavier_branch_arrives(&mut nodes, 3);
    assert_eq!(nodes[0].status().pool, 2);

    // A new block on the first node would keep them waiting if the second looked unspendable.
    mine(&mut nodes[0], 800_000);
    assert_eq!(nodes[0].chain().find_tx(&first), Some(7));
    assert_eq!(nodes[0].chain().find_tx(&second), Some(7));
}

#[test]
fn a_full_pool_stays_within_its_limit() {
    const POOL_LIMIT: usize = 256;
    let mut nodes = network(POOL_LIMIT as u64 + 3);
    let to = nodes[1].address();
    let mined = nodes[0].pay(to, AMOUNT, 0).unwrap();
    deliver(&mut nodes[..2]);
    // The other node mines it, so no waiting payment spends a reward that is about to vanish.
    mine(&mut nodes[1], 10_000_000);
    deliver(&mut nodes[..2]);
    let mut last = mined;
    for _ in 0..POOL_LIMIT {
        last = nodes[0].pay(to, AMOUNT, 0).unwrap();
    }
    assert_eq!(nodes[0].status().pool, POOL_LIMIT);
    take_events(&mut nodes[0]);

    heavier_branch_arrives(&mut nodes, 2);

    assert_eq!(nodes[0].status().pool, POOL_LIMIT);
    assert!(nodes[0].in_pool(&mined));
    assert!(!nodes[0].in_pool(&last));
    assert_eq!(dropped(&take_events(&mut nodes[0]), last), Some(None));
}

#[test]
fn a_reward_too_young_for_the_new_height_is_refused() {
    let mut rich = engine_with_maturity(1, 3);
    let mut short = engine_with_maturity(1, 3);
    for block in 0..3 {
        mine(&mut rich, block * 1_000);
    }
    let block = rich.chain().best_chain_from(1)[0].clone();
    short.restore_block(block);
    let to = engine(2).address();
    rich.pay(to, AMOUNT, 0).unwrap();
    mine(&mut rich, 5_000);
    let tx = rich.chain().best_chain_from(4)[0].txs[1].clone();

    let (kept, refused) = short.chain().recheck(vec![tx]);

    assert!(kept.is_empty());
    assert_eq!(refused[0].1, ChainError::ImmatureCoinbase);
}
