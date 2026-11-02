//! Catching up: forward from the node's own position, whatever arrives or gets lost.
mod common;

use common::{deliver, engine, engine_of, mine, silence, test_coin};
use olc_node::{Engine, Heights, Message, Outgoing};

const LONG_CHAIN: u64 = 200;
const HEAVY_ATTEMPTS: u32 = 1_000_000;
const BLOCKS_AT_ONCE: usize = 8;

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
fn a_block_damaged_on_the_way_comes_again() {
    let mut nodes = [engine(1), engine(2)];
    grown_by(&mut nodes[0], 30, 0);
    nodes[0].tick(100_000);
    let mut blocks_seen = 0;
    let mut damaged = None;
    carry(&mut nodes, |message| {
        let Message::Block(block) = message else {
            return false;
        };
        blocks_seen += 1;
        if blocks_seen == 3 {
            damaged = Some(block.clone());
        }
        blocks_seen == 3
    });
    let mut damaged = damaged.unwrap();
    damaged.header.merkle_root[0] ^= 1;
    nodes[1].on_message(0, Message::Block(damaged));
    let mut now = 110_000;
    for _ in 0..20 {
        nodes[0].tick(now);
        nodes[1].tick(now);
        carry(&mut nodes, |_| false);
        now += 2_000;
    }
    assert_eq!(nodes[1].status().tip, nodes[0].status().tip);
}

/// A board takes seconds over a block with many inputs, and asking again then only sends the
/// same blocks twice: on a real board that was five copies of every block.
#[test]
fn an_ask_repeated_before_the_answer_left_is_answered_once() {
    let mut node = engine(1);
    grown_by(&mut node, 30, 0);
    let genesis = node.chain().id_at(0).unwrap();
    let ask = || Message::GetBlocks(vec![genesis], Heights(vec![0]));
    let blocks = |node: &mut Engine| {
        std::iter::from_fn(|| node.take_outgoing())
            .filter(|out| matches!(out, Outgoing::To(_, Message::Block(..))))
            .count()
    };
    node.tick(100_000);
    blocks(&mut node);
    node.on_message(7, ask());
    assert_eq!(blocks(&mut node), BLOCKS_AT_ONCE);
    node.on_message(7, ask());
    assert_eq!(blocks(&mut node), 0);
    node.tick(103_000);
    node.on_message(7, ask());
    assert_eq!(blocks(&mut node), BLOCKS_AT_ONCE);
}

#[test]
fn a_slow_check_is_not_taken_for_a_quiet_peer() {
    let mut nodes = [engine(1), engine(2)];
    grown_by(&mut nodes[0], 30, 0);
    nodes[0].tick(100_000);
    nodes[1].tick(100_000);
    let mut held = Vec::new();
    carry(&mut nodes, |message| {
        let Message::Block(block) = message else {
            return false;
        };
        held.push(block.clone());
        true
    });
    assert_eq!(held.len(), BLOCKS_AT_ONCE);
    let asks = |node: &mut Engine| {
        std::iter::from_fn(|| node.take_outgoing())
            .filter(|out| matches!(out, Outgoing::To(_, Message::GetBlocks(..))))
            .count()
    };
    let last = held.pop().unwrap();
    for block in held {
        nodes[1].on_message(0, Message::Block(block));
        nodes[1].tick(105_000);
        assert_eq!(asks(&mut nodes[1]), 0);
    }
    nodes[1].tick(105_500);
    assert_eq!(asks(&mut nodes[1]), 0);
    nodes[1].on_message(0, Message::Block(last));
    assert_eq!(asks(&mut nodes[1]), 1);
}

/// Loses every block but a few, as a line that fails after real blocks came.
fn past_the_first_blocks() -> impl FnMut(&Message) -> bool {
    let mut let_through = 2;
    move |message| {
        if !matches!(message, Message::Block(_)) {
            return false;
        }
        let lost = let_through == 0;
        let_through -= usize::from(!lost);
        lost
    }
}

#[test]
fn a_node_well_behind_does_not_mine_until_it_has_caught_up() {
    let mut nodes = [engine(1), engine(2)];
    grown_by(&mut nodes[0], 30, 0);
    nodes[0].tick(100_000);
    nodes[1].tick(100_000);
    carry(&mut nodes, past_the_first_blocks());
    nodes[1].tick(100_500);
    assert_eq!(nodes[1].status().syncing_to, Some(30));
    assert!(!nodes[1].mine(HEAVY_ATTEMPTS, 100_500));

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
    carry(&mut nodes, past_the_first_blocks());
    nodes[1].tick(100_500);
    assert!(nodes[1].status().syncing_to.is_some());

    nodes[1].tick(200_000);
    assert_eq!(nodes[1].status().syncing_to, None);
    assert!(nodes[1].mine(HEAVY_ATTEMPTS, 200_000));
}

fn config_block_ms() -> u64 {
    test_coin().rules.block_time_ms
}

/// Offline groups run apart for hours, and when they meet the most work must win everywhere.
#[test]
fn nodes_that_mined_apart_for_long_end_on_one_chain() {
    let mut config = test_coin();
    config.rules.retarget_interval = 10;
    let born = config.genesis().header.time_ms;
    let mut nodes = [engine_of(1, config.clone()), engine_of(2, config)];
    for (node, blocks) in nodes.iter_mut().zip([300, 200]) {
        for block in 1..=blocks {
            mine(node, born + block * config_block_ms());
        }
        silence(node);
    }
    let mut now = born + 301 * config_block_ms();
    for _ in 0..80 {
        now += 2_000;
        nodes[0].tick(now);
        nodes[1].tick(now);
        carry(&mut nodes, |_| false);
    }
    assert_eq!(nodes[1].chain().tip(), nodes[0].chain().tip());
    assert_eq!(nodes[1].chain().height(), 300);
}

/// One false word from a peer must not stop a miner, however often it comes.
#[test]
fn a_claim_of_more_work_asks_and_stops_no_miner() {
    let mut node = engine(1);
    let genesis = node.chain().genesis_id();
    let claim = || Message::Tip {
        genesis,
        tip: [7; 32],
        previous: [6; 32],
        height: 1_000,
        work: u128::MAX,
    };
    let mut asked = 0;
    for second in 0..60 {
        let now = 100_000 + second * 1_000;
        node.tick(now);
        node.on_message(7, claim());
        asked += std::iter::from_fn(|| node.take_outgoing())
            .filter(|out| matches!(out, Outgoing::To(7, Message::GetBlocks(..))))
            .count();
        assert_eq!(node.status().syncing_to, None);
        assert!(node.mine(HEAVY_ATTEMPTS, now), "stopped at {second} s");
    }
    assert!(asked > 0);
}

/// Claiming the most work must not keep a peer with real blocks from being heard.
#[test]
fn a_peer_that_claims_and_brings_nothing_gives_way_to_one_that_brings_blocks() {
    const LIAR: u32 = 7;
    let mut nodes = [engine(1), engine(2)];
    grown_by(&mut nodes[0], 30, 0);
    let genesis = nodes[1].chain().genesis_id();
    for second in 0..40 {
        let now = 100_000 + second * 1_000;
        nodes[0].tick(now);
        nodes[1].tick(now);
        let claim = Message::Tip {
            genesis,
            tip: [7; 32],
            previous: [6; 32],
            height: 1_000,
            work: u128::MAX,
        };
        nodes[1].on_message(LIAR, claim);
        let mut moved = true;
        while moved {
            moved = false;
            while let Some(outgoing) = nodes[0].take_outgoing() {
                moved = true;
                let (Outgoing::To(_, message) | Outgoing::All { message, .. }) = outgoing;
                nodes[1].on_message(0, message);
            }
            while let Some(outgoing) = nodes[1].take_outgoing() {
                moved = true;
                let (to, message) = match outgoing {
                    Outgoing::To(to, message) => (to, message),
                    Outgoing::All { message, .. } => (0, message),
                };
                if to != LIAR {
                    nodes[0].on_message(1, message);
                }
            }
        }
    }
    assert_eq!(nodes[1].chain().tip(), nodes[0].chain().tip());
}
