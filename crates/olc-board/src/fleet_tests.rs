//! The fleet's tests: what it keeps of what boards say, and the packets it sends.
use super::*;
use crate::control_key::ControlKey;
use olc_control::sign_beacon;
use olc_crypto::Signer;

const LINK_SHIFT: u32 = 24;
const BOARD: BoardId = [1, 2, 3, 4, 5, 6];
const CABLE: u32 = 1 << LINK_SHIFT;
const SECRET: [u8; 32] = [3; 32];

fn link_of(peer: u32) -> usize {
    (peer >> LINK_SHIFT) as usize
}

fn fleet(name: &str) -> Fleet {
    let path = std::env::temp_dir().join(format!("olc-fleet-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&path).unwrap();
    let key = ControlKey::load_or_create(&path).unwrap();
    std::fs::remove_dir_all(&path).unwrap();
    Fleet::new(key, link_of).unwrap()
}

fn beacon(id: u32, hops: u8, from: BoardId) -> Vec<u8> {
    let signer = Signer::new(&SECRET).unwrap();
    let beacon = Beacon {
        name: "one".into(),
        address: olc_crypto::address_of(signer.public_key()),
        height: 9,
        tip: [0; 32],
        balance: 0,
        hashrate: 1,
        limit: 0,
        peers: 0,
        pool: 0,
        mining: true,
        offline: false,
        view: "basic".into(),
        language: "en".into(),
        build: "x".into(),
        slot: 0,
        counter: 41,
        trusts_a_key: true,
        refused: 0,
        ..Beacon::default()
    };
    Packet {
        from,
        to: None,
        id,
        hops,
        body: Body::Beacon(sign_beacon(&signer, &from, beacon).unwrap()),
    }
    .encode()
}

/// The bytes a build from before `smooth` and `genesis` sends: the fields it had, then a
/// signature in a place today's format reads as something else.
fn old_frame(from: BoardId, hops: u8) -> Vec<u8> {
    let mut frame = OldBeacon::decode(&beacon(1, hops, from)).unwrap().encode();
    frame.extend_from_slice(&[0, 0, 0]);
    frame
}

#[test]
fn made_up_boards_stay_on_a_short_list() {
    let mut fleet = fleet("made-up");
    for number in 0..2_000u32 {
        let mut from = [9; 6];
        from[..4].copy_from_slice(&number.to_le_bytes());
        let session = number;
        let bodies = [
            Body::Line(format!("done {number}")),
            Body::Refused(Refusal::Replayed),
            Body::Progress { session, next: 1 },
            Body::Missing { session, done: 0, runs: Vec::new() },
        ];
        for (at, body) in bodies.into_iter().enumerate() {
            let id = number * 4 + at as u32;
            fleet.hear(1, &Packet { from, to: None, id, hops: 0, body }.encode());
        }
        fleet.hear(1, &old_frame(from, 1));
    }
    assert!(fleet.boards.len() <= MOST_BOARDS, "{} boards", fleet.boards.len());
    assert!(fleet.lines.len() <= MOST_BOARDS, "{} line lists", fleet.lines.len());
    assert!(fleet.finished.len() <= MOST_BOARDS);
    assert!(fleet.refusals.len() <= MOST_BOARDS);
    assert!(fleet.progress.len() <= MOST_BOARDS);
    assert!(fleet.missing.len() <= MOST_BOARDS);

    let from = [8; 6];
    for id in 0..2_000 {
        let body = Body::Refused(Refusal::Replayed);
        fleet.hear(1, &Packet { from, to: None, id, hops: 0, body }.encode());
    }
    assert!(fleet.refusals[&from].len() <= MAX_LINES);
}

#[test]
fn made_up_old_boards_cannot_push_out_a_proved_one() {
    let mut fleet = fleet("pushed");
    fleet.hear(1, &beacon(1, 1, BOARD));
    for number in 0..2_000u32 {
        let mut from = [7; 6];
        from[..4].copy_from_slice(&number.to_le_bytes());
        fleet.hear(1, &old_frame(from, 1));
    }
    assert!(fleet.boards.contains_key(&BOARD));
    assert!(!fleet.boards[&BOARD].old_format);
}

#[test]
fn a_board_too_old_to_read_still_shows() {
    let mut fleet = fleet("old-board");
    fleet.hear(1, &old_frame(BOARD, 1));
    let boards = fleet.boards();
    assert_eq!(boards.len(), 1);
    assert!(boards[0].old_format);
    assert_eq!(boards[0].beacon.name, "one");
    assert_eq!(boards[0].beacon.build, "x");
    assert_eq!(boards[0].beacon.height, 9);
}

#[test]
fn old_words_cannot_change_a_board_that_proved_itself() {
    let mut fleet = fleet("old-words");
    fleet.hear(1, &beacon(1, 1, BOARD));
    let mut frame = OldBeacon::decode(&beacon(2, 1, BOARD)).unwrap();
    frame.name = "fake".into();
    fleet.hear(1, &frame.encode());
    let boards = fleet.boards();
    assert_eq!(boards[0].beacon.name, "one");
    assert!(!boards[0].old_format);
}

#[test]
fn a_beacon_nobody_signed_shows_no_board() {
    let mut fleet = fleet("forged");
    let mut packet = Packet::decode(&beacon(1, 1, BOARD)).unwrap();
    if let Body::Beacon(signed) = &mut packet.body {
        signed.beacon.height = 10;
    }
    fleet.hear(CABLE, &packet.encode());
    assert!(fleet.boards().is_empty());
}

#[test]
fn another_key_cannot_take_over_a_board_by_radio() {
    let mut fleet = fleet("taken");
    fleet.hear(CABLE, &beacon(1, 1, BOARD));
    let other = Signer::new(&[9; 32]).unwrap();
    let forged = Beacon {
        address: olc_crypto::address_of(other.public_key()),
        height: 99,
        ..Beacon::default()
    };
    let forged = Packet {
        from: BOARD,
        to: None,
        id: 2,
        hops: 1,
        body: Body::Beacon(sign_beacon(&other, &BOARD, forged).unwrap()),
    };
    fleet.hear(CABLE, &forged.encode());
    assert_eq!(fleet.boards()[0].beacon.height, 9);
}

#[test]
fn a_board_on_the_cable_is_direct_and_one_behind_it_is_not() {
    let mut fleet = fleet("direct");
    fleet.hear(CABLE, &beacon(1, 0, BOARD));
    let far = [9; 6];
    fleet.hear(CABLE, &beacon(2, 1, far));
    let boards = fleet.boards();
    let find = |id| boards.iter().find(|seen| seen.id == id).unwrap();
    assert_eq!(find(BOARD).connection, Connection::Direct { link: 1 });
    assert_eq!(
        find(far).connection,
        Connection::Through { board: Some(BOARD) }
    );
}

#[test]
fn a_direct_copy_counts_even_after_the_radio_copy_came_first() {
    let mut fleet = fleet("copies");
    fleet.hear(CABLE, &beacon(1, 0, BOARD));
    fleet.hear(CABLE, &beacon(2, 1, BOARD));
    fleet.hear(CABLE, &beacon(2, 0, BOARD));
    let boards = fleet.boards();
    assert_eq!(boards[0].connection, Connection::Direct { link: 1 });
}

#[test]
fn orders_count_up_from_what_the_board_showed() {
    let mut fleet = fleet("counter");
    fleet.hear(CABLE, &beacon(1, 0, BOARD));
    let first = fleet.sign(BOARD, Action::Line("status".into())).unwrap();
    let second = fleet.sign(BOARD, Action::Line("status".into())).unwrap();
    assert_eq!(first.order.counter, 42);
    assert_eq!(second.order.counter, 43);
}
