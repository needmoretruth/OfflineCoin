//! Catching up: forward from the node's own position, whatever arrives or gets lost.
mod common;

use common::{deliver, engine, mine, silence};
use olc_node::{Engine, Message, Outgoing};

const LONG_CHAIN: u64 = 200;
const HEAVY_ATTEMPTS: u32 = 1_000_000;

/// Passes everything from `from` to `to` and back, dropping what `lost` picks, and counts requests.
fn carry(nodes: &mut [Engine; 2], mut lost: impl FnMut(&Message) -> bool) -> usize {
    let mut asked = 0;
    let mut moved = true;
    while moved {
        moved = false;
        for sender in 0..2 {
            while let Some(outgoing) = nodes[sender].take_outgoing() {
                moved = true;
                let (Outgoing::To(_, message) | Outgoing::All { message, .. }) = outgoing;
                asked += usize::from(matches!(message, Message::GetBlocks(..)));
                if !lost(&message) {
                    nodes[1 - sender].on_message(sender as u32, message);
                }
            }
        }
    }
    asked
}

fn grown_by(node: &mut Engine, blocks: u64, first_ms: u64) {
    for block in 0..blocks {
        mine(node, first_ms + block * 1_000);
    }
    silence(node);
}

#[test]
fn a_new_node_walks_forward_and_never_waits_on_a_parent() {
    let mut nodes = [engine(1), engine(2)];
    grown_by(&mut nodes[0], LONG_CHAIN, 0);
    nodes[0].tick(LONG_CHAIN * 1_000);
    let mut out_of_order = false;
    let mut last_height = 0;
    let asked = carry(&mut nodes, |_| false);
    for block in nodes[1].chain().best_chain_from(1) {
        let height = block.txs[0].coinbase_height.unwrap();
        out_of_order |= height != last_height + 1;
        last_height = height;
    }
    assert!(!out_of_order);
    assert_eq!(last_height, LONG_CHAIN);
    assert_eq!(nodes[1].status().tip, nodes[0].status().tip);
    assert!(asked <= 1 + LONG_CHAIN as usize / 8 + 1, "{asked} requests");
}

#[test]
fn a_node_on_another_branch_finds_the_fork() {
    let mut nodes = [engine(1), engine(2)];
    grown_by(&mut nodes[0], 30, 0);
    nodes[0].tick(100_000);
    carry(&mut nodes, |_| false);
    grown_by(&mut nodes[1], 3, 200_000);
    grown_by(&mut nodes[0], 20, 300_000);
    nodes[0].tick(400_000);
    carry(&mut nodes, |_| false);
    assert_eq!(nodes[1].status().tip, nodes[0].status().tip);
    assert_eq!(nodes[1].status().height, 50);
}

#[test]
fn a_block_lost_on_the_way_comes_again() {
    let mut nodes = [engine(1), engine(2)];
    grown_by(&mut nodes[0], 30, 0);
    nodes[0].tick(100_000);
    let mut blocks_seen = 0;
    carry(&mut nodes, |message| {
        blocks_seen += usize::from(matches!(message, Message::Block(_)));
        blocks_seen == 3 && matches!(message, Message::Block(_))
    });
    let mut now = 110_000;
    for _ in 0..20 {
        nodes[0].tick(now);
        nodes[1].tick(now);
        carry(&mut nodes, |_| false);
        now += 2_000;
    }
    assert_eq!(nodes[1].status().tip, nodes[0].status().tip);
}

#[test]
fn a_node_well_behind_does_not_mine_until_it_has_caught_up() {
    let mut nodes = [engine(1), engine(2)];
    grown_by(&mut nodes[0], 30, 0);
    nodes[0].tick(100_000);
    nodes[1].tick(100_000);
    carry(&mut nodes, |message| matches!(message, Message::Block(_)));
    assert_eq!(nodes[1].status().syncing_to, Some(30));
    assert!(!nodes[1].mine(HEAVY_ATTEMPTS, 101_000));

    deliver(&mut nodes);
    nodes[0].tick(110_000);
    nodes[1].tick(110_000);
    carry(&mut nodes, |_| false);
    assert_eq!(nodes[1].status().height, 30);
    assert_eq!(nodes[1].status().syncing_to, None);
    assert!(nodes[1].mine(HEAVY_ATTEMPTS, 111_000));
}

#[test]
fn a_node_goes_back_to_mining_when_the_peer_it_followed_leaves() {
    let mut nodes = [engine(1), engine(2)];
    grown_by(&mut nodes[0], 30, 0);
    nodes[0].tick(100_000);
    nodes[1].tick(100_000);
    carry(&mut nodes, |message| matches!(message, Message::Block(_)));
    assert!(nodes[1].status().syncing_to.is_some());

    nodes[1].tick(200_000);
    assert_eq!(nodes[1].status().syncing_to, None);
    assert!(nodes[1].mine(HEAVY_ATTEMPTS, 200_000));
}
