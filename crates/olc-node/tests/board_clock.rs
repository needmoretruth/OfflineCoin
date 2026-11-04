//! A board's clock and the date: a board counts from its own start, so it takes the date from the
//! app that controls it and from what it saved before a restart, never earlier than its tip. And a
//! node that starts far behind takes the chain quickly before it mines.
mod common;
use common::{engine, mine, silence};
use olc_node::{Engine, Message, Outgoing};

/// A board's uptime when it starts mining.
const BOOT_MS: u64 = 5_000;
const BLOCK_MS: u64 = 30_000;

fn genesis_time(node: &Engine) -> u64 {
    node.chain().tip_header().time_ms
}

#[test]
fn an_undated_board_stamps_from_its_genesis_and_from_the_date_once_given() {
    let mut board = engine(1);
    let genesis = genesis_time(&board);
    let date = genesis + 26 * 3_600_000;
    board.tick(BOOT_MS);
    mine(&mut board, BOOT_MS + BLOCK_MS);
    let undated = board.chain().tip_header().time_ms;
    assert!(undated < genesis + 2 * BLOCK_MS, "the cause: a board starts from its genesis");

    assert!(board.take_date(date, BOOT_MS + BLOCK_MS));
    assert_eq!(board.clock_at(BOOT_MS + BLOCK_MS), date);
    mine(&mut board, BOOT_MS + 2 * BLOCK_MS);
    assert_eq!(board.chain().tip_header().time_ms, (date + BLOCK_MS) / 1_000 * 1_000);
    // An older copy of a signed time, or a time before the tip, moves nothing back.
    assert!(!board.take_date(date - 3_600_000, BOOT_MS + 2 * BLOCK_MS));
    assert!(!board.take_date(genesis, BOOT_MS + 2 * BLOCK_MS));
    assert_eq!(board.clock_at(BOOT_MS + 2 * BLOCK_MS), date + BLOCK_MS);
    // Nor does a time that is no date at all.
    assert!(!board.take_date(BOOT_MS, BOOT_MS + 2 * BLOCK_MS));
}

#[test]
fn a_board_starting_from_a_saved_date_never_goes_below_its_tip() {
    let mut board = engine(1);
    let tip = genesis_time(&board);
    board.take_date(tip - 600_000, BOOT_MS);
    board.tick(BOOT_MS);
    assert!(board.clock_at(BOOT_MS) >= tip);
}

#[test]
fn a_computer_takes_no_date_from_anyone() {
    let mut computer = engine(1);
    let now = genesis_time(&computer) + BLOCK_MS;
    computer.tick(now);
    assert!(!computer.take_date(now + 3_600_000, now));
    assert_eq!(computer.clock_at(now), now);
}

/// Bitcoin Core mines nothing in its initial download. A computer that starts on a tip hours old
/// gives its peers a moment to say what they hold, so its first block is not one at the floor on a
/// tip they left long ago. A computer alone mines after the moment.
#[test]
fn a_computer_on_an_old_tip_hears_its_peers_before_it_mines() {
    let mut computer = engine(1);
    let started = genesis_time(&computer) + 26 * 3_600_000;
    computer.tick(started);
    assert!(!computer.mine(1_000_000, started), "mined at once on a day-old tip");
    computer.tick(started + 9_000);
    assert!(!computer.mine(1_000_000, started + 9_000));
    computer.tick(started + 10_000);
    assert!(computer.mine(1_000_000, started + 10_000), "a computer alone mines after a moment");

    let mut fresh = engine(2);
    let now = genesis_time(&fresh) + BLOCK_MS;
    fresh.tick(now);
    assert!(fresh.mine(1_000_000, now), "a fresh tip mines at once");
}

/// Carries messages between two nodes in trips: what each sends in one trip arrives in the next,
/// as over a wire with a delay, a cable to a computer or TCP when `fast`. Says how many trips
/// the chain took. Every `lose`th block sent goes missing; a second passes every 50 trips.
fn sync(nodes: &mut [Engine; 2], fast: bool, lose: usize) -> usize {
    let mut sent = 0;
    let mut now = nodes[0].chain().tip_header().time_ms + BLOCK_MS;
    for trip in 1..20_000 {
        let mut on_the_way = Vec::new();
        for (from, node) in nodes.iter_mut().enumerate() {
            while let Some(outgoing) = node.take_outgoing() {
                let (Outgoing::To(_, message) | Outgoing::All { message, .. }) = outgoing;
                if matches!(message, Message::Block(..)) {
                    sent += 1;
                    if lose > 0 && sent % lose == 0 {
                        continue;
                    }
                }
                on_the_way.push((from, message));
            }
        }
        for (from, message) in on_the_way {
            nodes[1 - from].on_message_over(from as u32, message, fast);
        }
        if nodes[1].chain().height() == nodes[0].chain().height() {
            return trip;
        }
        if trip % 50 == 0 {
            now += 1_000;
            nodes[0].tick(now);
            nodes[1].tick(now);
        }
    }
    panic!("height {} of {}", nodes[1].chain().height(), nodes[0].chain().height());
}

fn chain_of(blocks: u64) -> Engine {
    let mut node = engine(1);
    let start = genesis_time(&node);
    for height in 1..=blocks {
        mine(&mut node, start + height * BLOCK_MS);
    }
    silence(&mut node);
    node
}

#[test]
fn a_node_far_behind_takes_the_chain_in_few_trips_over_a_fast_link() {
    const BLOCKS: u64 = 1_500;
    let start = |node: Engine| {
        let mut nodes = [node, engine(2)];
        let now = nodes[0].chain().tip_header().time_ms + BLOCK_MS;
        nodes[0].tick(now);
        nodes[1].tick(now);
        nodes
    };
    let slow = sync(&mut start(chain_of(BLOCKS)), false, 0);
    let fast = sync(&mut start(chain_of(BLOCKS)), true, 0);
    println!("{BLOCKS} blocks in {slow} trips by radio, {fast} over a fast link");
    assert!(fast * 4 < slow, "{fast} trips against {slow}");
    // Blocks lost on the way are asked for again.
    let lossy = sync(&mut start(chain_of(BLOCKS)), true, 97);
    println!("{lossy} trips with every 97th block lost");
}
