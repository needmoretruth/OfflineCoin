//! Several networks on one radio channel or LAN: other coins, the same genesis with other rules,
//! and groups of one coin that grew apart.
mod common;

use common::{deliver, engine, engine_of, mine, silence, test_coin};
use olc_node::{Engine, Event, Link, Message, Outgoing, Runner, Target};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

const ANNOUNCE_MS: u64 = 2_000;
const MAX_PASSES: usize = 100;

type Queues = Arc<Mutex<Vec<VecDeque<(u32, Vec<u8>)>>>>;

/// One shared channel: whatever a node sends, everyone else hears.
struct Air {
    me: usize,
    queues: Queues,
}

impl Link for Air {
    fn medium(&self) -> olc_node::Medium {
        olc_node::Medium::EspNow
    }

    fn send(&mut self, _target: Target, message: &[u8]) {
        let mut queues = self.queues.lock().unwrap();
        for (node, queue) in queues.iter_mut().enumerate() {
            if node != self.me {
                queue.push_back((self.me as u32, message.to_vec()));
            }
        }
    }

    fn receive(&mut self) -> Option<(u32, Vec<u8>)> {
        self.queues.lock().unwrap()[self.me].pop_front()
    }
}

fn on_the_air(engines: Vec<Engine>) -> Vec<Runner> {
    let queues: Queues = Arc::new(Mutex::new(vec![VecDeque::new(); engines.len()]));
    let attach = |(me, engine)| {
        let mut runner = Runner::new(engine);
        runner.add_link(Box::new(Air {
            me,
            queues: queues.clone(),
        }));
        runner
    };
    engines.into_iter().enumerate().map(attach).collect()
}

fn other_coin(seed: u8) -> Engine {
    let mut config = test_coin();
    config.message = "another coin".into();
    engine_of(seed, config)
}

#[test]
fn another_coin_on_the_same_channel_leaves_no_trace() {
    let mut runners = on_the_air(vec![engine(1), other_coin(2)]);
    for block in 0..5 {
        for runner in &mut runners {
            mine(&mut runner.engine, block * ANNOUNCE_MS);
            while runner.engine.take_event().is_some() {}
        }
        for _ in 0..3 {
            for runner in &mut runners {
                runner.step(block * ANNOUNCE_MS);
            }
        }
    }
    for runner in &mut runners {
        assert_eq!(runner.engine.peer_ids().count(), 0);
        assert_eq!(runner.engine.status().height, 5);
        assert!(runner.engine.take_event().is_none());
    }
}

#[test]
fn control_traffic_crosses_coins() {
    let mut runners = on_the_air(vec![engine(1), other_coin(2)]);
    let order = Message::Control(vec![1, 2, 3]);
    runners[0].send_now(Outgoing::All {
        except: None,
        message: order,
    });
    runners[1].step(0);
    assert_eq!(runners[1].take_control().unwrap().1, vec![1, 2, 3]);
    assert_eq!(runners[1].engine.peer_ids().count(), 0);
}

#[test]
fn one_coin_still_finds_itself_through_the_marks() {
    let mut runners = on_the_air(vec![engine(1), engine(2)]);
    mine(&mut runners[0].engine, 0);
    for now in [ANNOUNCE_MS, ANNOUNCE_MS, ANNOUNCE_MS] {
        for runner in &mut runners {
            runner.step(now);
        }
    }
    assert_eq!(runners[1].engine.status().height, 1);
    assert_eq!(runners[1].engine.peer_ids().count(), 1);
}

/// Carries messages between two nodes until both are quiet, and counts the blocks they asked for.
/// Two nodes that never go quiet stop at `MAX_PASSES`, and the count shows it.
fn exchange(nodes: &mut [Engine; 2]) -> usize {
    let mut requests = 0;
    let mut moved = true;
    let mut passes = 0;
    while moved && passes < MAX_PASSES {
        moved = false;
        passes += 1;
        for sender in 0..2 {
            while let Some(outgoing) = nodes[sender].take_outgoing() {
                moved = true;
                let (Outgoing::To(_, message) | Outgoing::All { message, .. }) = outgoing;
                requests += usize::from(matches!(message, Message::GetBlocks(..)));
                nodes[1 - sender].on_message(sender as u32, message);
            }
        }
    }
    requests
}

/// The same genesis, but each side raises the difficulty at heights the other does not expect.
fn rivals() -> [Engine; 2] {
    let (mut first, mut second) = (test_coin(), test_coin());
    first.rules.retarget_interval = 2;
    second.rules.retarget_interval = 3;
    [engine_of(1, first), engine_of(2, second)]
}

#[test]
fn rivals_on_one_genesis_stop_asking_for_blocks_they_refused() {
    let mut nodes = rivals();
    let mut settling = 0;
    for block in 1..=4 {
        mine(&mut nodes[0], block * 1_000);
        settling += exchange(&mut nodes);
    }
    for block in 1..=6 {
        mine(&mut nodes[1], block * 1_000);
        settling += exchange(&mut nodes);
    }
    assert!(settling <= 12, "{settling} requests to learn 10 blocks");
    let tips = [nodes[0].chain().tip(), nodes[1].chain().tip()];
    let mut repeated = 0;
    for round in 1..=30 {
        for node in &mut nodes {
            node.tick(10_000 + round * ANNOUNCE_MS);
        }
        repeated += exchange(&mut nodes);
    }
    assert_eq!(repeated, 0);
    assert_eq!([nodes[0].chain().tip(), nodes[1].chain().tip()], tips);
    assert_eq!(nodes[0].peer_ids().count(), 1);
    assert_eq!(nodes[1].peer_ids().count(), 1);
    let height = nodes[1].status().height;
    assert!(height >= 6, "the second node stayed on its own chain");
    let saw_a_refusal = std::iter::from_fn(|| nodes[0].take_event())
        .any(|event| matches!(event, Event::BlockRejected { .. }));
    assert!(saw_a_refusal);
    assert_eq!(nodes[0].status().syncing_to, None);
    assert!(
        nodes[0].mine(1_000_000, 100_000),
        "a taller rival must not stop a miner"
    );
}

#[test]
fn two_groups_of_one_coin_meet_and_most_work_wins() {
    let mut nodes = [engine(1), engine(2), engine(3), engine(4)];
    for block in 1..=2 {
        mine(&mut nodes[0], block * 1_000);
        deliver(&mut nodes[..2]);
    }
    for block in 1..=5 {
        mine(&mut nodes[2], block * 1_000);
        deliver(&mut nodes[2..]);
    }
    let heavier = nodes[2].chain().tip();
    nodes.iter_mut().for_each(silence);
    nodes[2].tick(10_000);
    nodes[3].tick(10_000);
    deliver(&mut nodes);
    assert!(nodes.iter().all(|node| node.chain().tip() == heavier));
}
