//! A waiting payment sped up: replaced by one paying more (replace-by-fee, as Bitcoin Core 28's
//! full replacement on BIP 125's fee terms), or pulled into a block by a child that pays for both
//! (child pays for parent).
mod common;

use common::{deliver, engine, mine};
use olc_core::{ChainError, Hash};
use olc_node::{Engine, Event};

fn rejected(node: &mut Engine) -> Vec<ChainError> {
    core::iter::from_fn(|| node.take_event())
        .filter_map(|event| match event {
            Event::TxRejected { reason } => Some(reason),
            _ => None,
        })
        .collect()
}

fn mined(node: &Engine, id: &Hash) -> bool {
    node.chain().find_tx(id).is_some()
}

#[test]
fn a_sped_up_payment_replaces_the_first_everywhere_and_is_mined_instead() {
    let mut nodes = [engine(1), engine(2)];
    for block in 0..3 {
        mine(&mut nodes[0], block * 1_000);
        deliver(&mut nodes);
    }
    let to = nodes[1].address();
    let first = nodes[0].pay(to, 5_000, 0).unwrap();
    deliver(&mut nodes);
    assert!(nodes[1].in_pool(&first));

    let sped = nodes[0].speed_up(&first).unwrap();
    assert!(sped.replaced && sped.fee > 0);
    deliver(&mut nodes);
    for node in &nodes {
        assert!(!node.in_pool(&first));
        assert!(node.in_pool(&sped.id));
    }
    mine(&mut nodes[1], 10_000);
    deliver(&mut nodes);
    assert!(mined(&nodes[0], &sped.id));
    assert!(!mined(&nodes[0], &first));
}

#[test]
fn a_replacement_must_pay_more_than_what_it_pushes_out() {
    let mut nodes = [engine(1), engine(1)];
    for block in 0..3 {
        mine(&mut nodes[0], block * 1_000);
        deliver(&mut nodes);
    }
    let to = engine(2).address();
    let first = nodes[0].pay(to, 5_000, 500).unwrap();
    // The other node never heard of it, and signs the same coin with the same fee.
    nodes[1].clear_pool();
    nodes[1].pay(to, 4_000, 500).unwrap();
    rejected(&mut nodes[0]);
    deliver(&mut nodes);
    assert!(nodes[0].in_pool(&first));
    assert!(rejected(&mut nodes[0]).contains(&ChainError::FeeTooLow));
}

#[test]
fn a_child_paying_well_takes_its_parent_into_a_full_block() {
    let mut nodes = [engine(1), engine(2)];
    for block in 0..40 {
        mine(&mut nodes[0], block * 1_000);
    }
    deliver(&mut nodes);
    let to = nodes[1].address();
    let parent = nodes[0].pay(to, 50_000, 0).unwrap();
    let others = engine(3).address();
    for _ in 0..30 {
        nodes[0].pay(others, 1_000, 1_000).unwrap();
    }
    deliver(&mut nodes);
    assert_eq!(nodes[1].pool().len(), 31);

    // Without the child the parent waits: the block fills with payments paying more.
    let chain = nodes[1].chain();
    let alone = chain.block_template(&to, nodes[1].pool(), 1_000_000);
    assert!(alone.txs.iter().all(|tx| tx.id() != parent));

    let child = nodes[1].speed_up(&parent).unwrap();
    assert!(!child.replaced);
    deliver(&mut nodes);
    mine(&mut nodes[0], 100_000);
    deliver(&mut nodes);
    assert!(mined(&nodes[1], &parent));
    assert!(mined(&nodes[1], &child.id));
    assert!(nodes[1].pool().len() > 1, "the block was full and others still wait");
}
