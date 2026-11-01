//! The table on its own: who gets chosen, when it moves, and when it stays.
use super::*;

const WIRE: usize = 0;
const RADIO: usize = 1;
const BLUETOOTH: usize = 2;

fn table() -> Neighbours {
    let mut table = Neighbours::default();
    for medium in [Medium::Wire, Medium::EspNow, Medium::Bluetooth] {
        table.add_link(medium);
    }
    table
}

fn run(table: &mut Neighbours, from: u64, until: u64, links: &[usize]) -> Vec<Move> {
    let mut moves = Vec::new();
    for now in (from..until).step_by(HELLO_MS as usize) {
        for link in links {
            table.name(*link, 7, 9, now);
        }
        moves.extend(table.settle(now));
    }
    moves
}

#[test]
fn a_neighbour_on_two_links_is_one_neighbour_with_one_link() {
    let mut table = table();
    run(&mut table, 0, 2_000, &[RADIO, BLUETOOTH]);
    assert_eq!(table.list().len(), 1);
    assert_eq!(table.count(), 1);
    assert_eq!(table.count_on(RADIO) + table.count_on(BLUETOOTH), 1);
}

#[test]
fn silence_moves_traffic_to_what_is_already_alive_within_a_few_hellos() {
    let mut table = table();
    run(&mut table, 0, 60_000, &[RADIO, BLUETOOTH]);
    assert_eq!(table.route(table.list()[0].peer), Some((RADIO, 7)));
    let moves = run(&mut table, 60_000, 60_000 + SILENT_MS + HELLO_MS, &[BLUETOOTH]);
    assert_eq!(moves.len(), 1);
    assert_eq!(moves[0].to, Some(BLUETOOTH));
}

#[test]
fn a_neighbour_silent_everywhere_stays_on_its_link_until_it_is_truly_gone() {
    let mut table = table();
    run(&mut table, 0, 60_000, &[WIRE, RADIO]);
    let peer = table.list()[0].peer;
    // A board signing a large payment stops every link at once for some seconds.
    let busy = run(&mut table, 60_000 + 8_000, 60_000 + 40_000, &[WIRE, RADIO]);
    assert_eq!((busy.len(), table.list()[0].moves), (0, 0), "{busy:?}");
    assert_eq!(table.route(peer), Some((WIRE, 7)));

    assert_eq!(table.settle(100_000 + SILENT_MS), []);
    assert_eq!(table.route(peer), Some((WIRE, 7)), "still on it, as it may come back");
    assert_eq!(table.fan(RADIO, None), Fan::Everyone, "the next word still reaches it");
    assert_eq!(table.fan(WIRE, None), Fan::Everyone);
    let gone = table.settle(99_000 + CLOSE_MS);
    assert_eq!(gone.len(), 1);
    assert_eq!((gone[0].to, table.count()), (None, 0));
}

#[test]
fn a_better_link_must_stay_heard_before_traffic_returns() {
    let mut table = table();
    run(&mut table, 0, 5_000, &[BLUETOOTH]);
    assert!(run(&mut table, 5_000, 5_000 + STEADY_MS - HELLO_MS, &[RADIO, BLUETOOTH]).is_empty());
    let back = run(&mut table, 5_000 + STEADY_MS, 5_000 + STEADY_MS + 2 * HELLO_MS, &[RADIO, BLUETOOTH]);
    assert_eq!(back.len(), 1);
    assert_eq!(back[0].to, Some(RADIO));
}

#[test]
fn a_link_that_keeps_dropping_out_never_wins_traffic_back() {
    let mut table = table();
    run(&mut table, 0, 5_000, &[BLUETOOTH]);
    let mut moves = Vec::new();
    for round in 0..30 {
        let start = 5_000 + round * 25_000;
        moves.extend(run(&mut table, start, start + 20_000, &[RADIO, BLUETOOTH]));
        moves.extend(run(&mut table, start + 20_000, start + 25_000, &[BLUETOOTH]));
    }
    assert!(moves.is_empty(), "{moves:?}");
}

#[test]
fn a_closed_connection_moves_traffic_at_once() {
    let mut table = table();
    run(&mut table, 0, 60_000, &[WIRE, RADIO]);
    assert_eq!(table.count_on(WIRE), 1);
    table.lost(WIRE, 7);
    table.settle(60_000);
    assert_eq!(table.count_on(RADIO), 1);
}

#[test]
fn the_order_is_a_choice() {
    let mut table = table();
    table.rank = Rank::parse("bluetooth,espnow,wired,wifi").unwrap();
    run(&mut table, 0, 60_000, &[RADIO, BLUETOOTH]);
    assert_eq!(table.count_on(BLUETOOTH), 1);
}

#[test]
fn a_nameless_old_build_is_one_neighbour_per_link() {
    let mut table = table();
    table.hear(RADIO, 1, 0);
    table.hear(BLUETOOTH, 1, 0);
    table.settle(0);
    assert_eq!(table.count(), 2);
}

#[test]
fn naming_folds_a_stranger_into_the_neighbour_it_turns_out_to_be() {
    let mut table = table();
    table.name(RADIO, 1, 9, 0);
    let stranger = table.hear(BLUETOOTH, 2, 0);
    assert_eq!(table.name(BLUETOOTH, 2, 9, 0), Some(stranger));
    assert_eq!(table.list().len(), 1);
    assert_eq!(table.list()[0].paths.len(), 2);
}

#[test]
fn one_broadcast_reaches_everyone_active_on_a_link() {
    let mut table = table();
    table.name(RADIO, 1, 10, 0);
    table.name(RADIO, 2, 11, 0);
    table.settle(0);
    assert_eq!(table.fan(RADIO, None), Fan::Everyone);
}

#[test]
fn a_neighbour_active_elsewhere_is_left_out_of_a_broadcast() {
    let mut table = table();
    table.name(WIRE, 1, 10, 0);
    table.name(RADIO, 1, 10, 0);
    table.name(RADIO, 2, 11, 0);
    table.settle(0);
    assert_eq!(table.fan(RADIO, None), Fan::Each(alloc::vec![2]));
    assert_eq!(table.fan(WIRE, None), Fan::Everyone);
}

#[test]
fn the_sender_of_a_message_does_not_get_it_back() {
    let mut table = table();
    let from = table.hear(RADIO, 1, 0);
    table.name(RADIO, 2, 11, 0);
    table.settle(0);
    assert_eq!(table.fan(RADIO, Some(from)), Fan::EveryoneBut(1));
    assert_eq!(table.fan(BLUETOOTH, None), Fan::Everyone);
}
