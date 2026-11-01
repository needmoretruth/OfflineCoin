//! Nodes that dropped their old blocks, and the node that needs them. Real engines, real mining.
mod common;
mod store;

use common::{deliver, engine, mine, silence, test_coin};
use olc_core::Chain;
use olc_node::{Engine, Event, Message, Outgoing};
use olc_wallet::{Wallet, MERGE_FROM};
use store::{flash, keep_on, plugins, STATE_SIZE};

fn pruned_engine(seed: u8, blocks: u64) -> Engine {
    let drive = flash(STATE_SIZE * 2 + 128 * 1024);
    let chain = Chain::open(test_coin(), plugins(), keep_on(&drive, None));
    let wallet = Wallet::from_secret([seed; 32]).unwrap();
    let mut node = Engine::on_chain(chain, wallet);
    node.set_merging(false);
    for height in 0..blocks {
        mine(&mut node, height * 1_000);
        node.tick(height * 1_000);
        silence(&mut node);
    }
    node
}

fn events(node: &mut Engine) -> Vec<Event> {
    std::iter::from_fn(|| node.take_event()).collect()
}

/// Lets `nodes` talk with the clock running, as a real pair would.
fn talk(nodes: &mut [Engine], from_ms: u64, rounds: u64) {
    for round in 0..rounds {
        for node in nodes.iter_mut() {
            node.tick(from_ms + round * 2_100);
        }
        deliver(nodes);
    }
}

#[test]
fn a_new_node_hears_plainly_when_nobody_has_the_history() {
    let mut nodes = [pruned_engine(1, 2_400), engine(2)];
    let start = 2_400_000;
    assert!(nodes[0].chain().oldest_held() > 1);
    talk(&mut nodes, start, 3);

    assert_eq!(nodes[1].chain().height(), 0);
    let status = nodes[1].status();
    assert_eq!(status.needs_history, Some((0, nodes[0].chain().height())));
    assert!(events(&mut nodes[1]).contains(&Event::NeedsHistory { peer: 0 }));
    // Nothing happens by itself: the unspent set stays the node's own.
    assert_eq!(status.assumed_from, None);
}

#[test]
fn a_person_can_take_a_peers_snapshot_and_the_chain_says_whose_it_is() {
    let mut nodes = [pruned_engine(1, 2_400), engine(2)];
    let start = 2_400_000;
    talk(&mut nodes, start, 2);
    nodes[1].accept_snapshot(0);
    deliver(&mut nodes);

    assert_eq!(nodes[1].status().tip, nodes[0].status().tip);
    assert_eq!(nodes[1].chain().unspent_hash(), nodes[0].chain().unspent_hash());
    assert_eq!(nodes[1].status().assumed_from, Some(0));
    assert!(events(&mut nodes[1]).iter().any(|e| matches!(e, Event::SnapshotAdopted { .. })));

    // It mines on, and the other node takes its block as any other.
    mine(&mut nodes[1], start + 60_000);
    deliver(&mut nodes);
    assert_eq!(nodes[0].status().tip, nodes[1].status().tip);
}

#[test]
fn a_snapshot_that_lies_about_its_coins_is_refused() {
    let mut nodes = [pruned_engine(1, 2_400), engine(2)];
    nodes[1].accept_snapshot(0);
    // A part with the wrong contents fails the hash and changes nothing.
    while let Some(outgoing) = nodes[1].take_outgoing() {
        assert!(matches!(outgoing, Outgoing::To(0, Message::GetSnapshot { part: 0 })));
    }
    let lie = Message::SnapshotPart {
        height: 2_400,
        sum: [7; 32],
        part: 0,
        parts: 1,
        bytes: vec![1, 2, 3],
    };
    nodes[1].on_message(0, lie);
    assert_eq!(nodes[1].chain().height(), 0);
    assert!(events(&mut nodes[1]).iter().any(|e| matches!(e, Event::SnapshotRefused { .. })));
}

#[test]
fn a_peer_with_all_the_blocks_is_used_before_anything_else() {
    let mut full = engine(1);
    for height in 0..30 {
        mine(&mut full, height * 1_000);
    }
    let mut nodes = [full, engine(2)];
    talk(&mut nodes, 40_000, 6);
    assert_eq!(nodes[1].status().tip, nodes[0].status().tip);
    assert_eq!(nodes[1].status().needs_history, None);
}

#[test]
fn a_miners_small_coins_become_one() {
    let mut node = engine(1);
    node.set_merging(true);
    let mut merged = false;
    for height in 0..MERGE_FROM as u64 * 4 {
        mine(&mut node, height * 1_000);
        merged |= events(&mut node).iter().any(|e| matches!(e, Event::CoinsMerged { .. }));
    }
    assert!(merged);
    let address = node.address();
    let coins = node.chain().unspent_of(&address).len();
    assert!(coins <= MERGE_FROM + 1, "{coins} coins");
    let reward = node.chain().rules().reward_at(1);
    assert_eq!(node.status().balance, reward * MERGE_FROM as u64 * 4);
}

#[test]
fn with_merging_off_the_coins_stay_as_they_are() {
    let mut node = engine(1);
    for height in 0..MERGE_FROM as u64 * 2 {
        mine(&mut node, height * 1_000);
    }
    let address = node.address();
    assert_eq!(node.chain().unspent_of(&address).len(), MERGE_FROM * 2);
}

#[test]
fn one_miner_can_mine_to_any_address() {
    let shared = Wallet::from_secret([9; 32]).unwrap().address();
    let mut boards = [engine(1), engine(2), engine(3)];
    boards.iter_mut().for_each(|board| board.set_miner(Some(shared)));
    for round in 0..3 {
        mine(&mut boards[round], round as u64 * 1_000 + 1_000);
        deliver(&mut boards);
    }
    assert_eq!(boards[0].chain().unspent_of(&shared).len(), 3);
    let own = boards[0].wallet().address();
    assert_eq!(boards[0].chain().unspent_of(&own).len(), 0);
}

#[test]
fn a_note_goes_into_a_block_and_a_long_one_stays_out_of_the_pool() {
    let mut node = engine(1);
    for height in 0..3 {
        mine(&mut node, height * 1_000);
    }
    let long = vec![7; olc_node::MOST_DATA + 1];
    assert!(node.write_note(&long).is_ok());
    assert_eq!(node.status().pool, 0);
    assert!(events(&mut node).contains(&Event::DataRefused));
    let id = node.write_note(b"first note").unwrap();
    mine(&mut node, 9_000);
    assert_eq!(node.chain().find_tx(&id), Some(4));
    let block = node.chain().blocks_from(4, 1).remove(0);
    let data = block.txs[1].outputs.iter().find(|out| out.is_data()).unwrap();
    assert_eq!(data.lock, b"first note");
}
