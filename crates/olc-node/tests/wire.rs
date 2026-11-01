//! What this build says must still make sense to the build before it. `Older` is that build's
//! list of messages, frozen here, so a change on this side shows up as a failure.
mod common;

use common::{engine_of, mine, silence, test_coin};
use olc_core::{Block, Hash, Tx};
use olc_node::{Message, Outgoing};
use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Serialize, Deserialize)]
enum Older {
    Tip {
        genesis: Hash,
        tip: Hash,
        previous: Hash,
        height: u64,
        work: u128,
    },
    GetBlocks(Vec<Hash>),
    Block(Block),
    Tx(Tx),
    Control(Vec<u8>),
}

const SPACED_MS: u64 = 30_000;
const CHAIN: u64 = 900;

#[test]
fn an_older_build_reads_a_request_for_blocks() {
    let ids = vec![[1; 32], [2; 32]];
    let bytes = Message::GetBlocks(ids.clone(), olc_node::Heights(vec![9, 8])).encode();
    assert_eq!(postcard::from_bytes::<Older>(&bytes).unwrap(), Older::GetBlocks(ids));
}

#[test]
fn an_older_build_reads_blocks_and_tips() {
    let (genesis, tip, previous) = ([1; 32], [2; 32], [3; 32]);
    let (height, work) = (5, 6);
    let new = Message::Tip { genesis, tip, previous, height, work }.encode();
    let old = Older::Tip { genesis, tip, previous, height, work };
    assert_eq!(new, postcard::to_allocvec(&old).unwrap());
}

/// A peer far behind, naming only ids, still moves forward from a node that keeps few in memory.
#[test]
fn a_far_behind_older_build_keeps_catching_up() {
    let mut node = engine_of(1, test_coin());
    let mut time = node.chain().tip_header().time_ms;
    let mut ids = vec![node.chain().genesis_id()];
    for _ in 0..CHAIN {
        time += SPACED_MS;
        mine(&mut node, time);
        silence(&mut node);
    }
    let mut got = 0;
    while got < CHAIN {
        let asked = postcard::to_allocvec(&Older::GetBlocks(ids.clone())).unwrap();
        node.on_message(7, Message::decode(&asked).unwrap());
        let mut answered = false;
        while let Some(out) = node.take_outgoing() {
            if let Outgoing::To(7, Message::Block(block)) = out {
                got += 1;
                answered = true;
                assert_eq!(node.chain().id_at(got), Some(block.id()));
                ids.insert(0, block.id());
            }
        }
        assert!(answered, "stuck at {got}");
        ids.truncate(8);
        ids.push(node.chain().genesis_id());
    }
}
