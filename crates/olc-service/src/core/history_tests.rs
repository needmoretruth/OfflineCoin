use super::*;
use olc_core::{TxIn, TxOut, SEQUENCE_FINAL};

const A: Address = [1; 20];
const B: Address = [2; 20];
const STRANGER: Address = [9; 20];

fn point(n: u8) -> OutPoint {
    OutPoint { txid: [n; 32], index: 0 }
}

fn pay(from: &[u8], to: &[(Address, u64)]) -> Tx {
    let input = |n: &u8| TxIn { previous: point(*n), unlock: Vec::new(), sequence: SEQUENCE_FINAL };
    let output = |(to, value): &(Address, u64)| TxOut { value: *value, language: olc_lang::ALPHA, lock: to.to_vec() };
    Tx { version: 2, inputs: from.iter().map(input).collect(), outputs: to.iter().map(output).collect(), coinbase_height: None, lock_time: 0 }
}

/// The wallet owns A and B. Coins 1 to 8 are A's at 50 each, coin 9 is B's.
fn shape(tx: &Tx) -> Shape {
    let owner = |point: &OutPoint| match point.txid[0] {
        1..=8 => Some((A, 50)),
        9 => Some((B, 50)),
        _ => None,
    };
    shape_of(tx, &[A, B], owner)
}

/// What the owner saw while mining: eight coins of 50 became one of 399, and it read as "To
/// yourself 400". It is a join with its fee.
#[test]
fn eight_mined_coins_made_one_read_as_a_join_with_its_fee() {
    let merge = shape(&pay(&[1, 2, 3, 4, 5, 6, 7, 8], &[(A, 399)]));
    assert_eq!(kind_of(&merge, 9), Some((ActivityKind::Joined, 399)));
    assert_eq!(merge.fee(), Some(1));
    assert_eq!(merge.spent.len(), 8);
}

#[test]
fn coins_moved_to_another_address_of_mine_are_not_a_join() {
    let moved = shape(&pay(&[1, 2], &[(B, 99)]));
    assert_eq!(kind_of(&moved, 9), Some((ActivityKind::ToSelf, 99)));
    let one_coin = shape(&pay(&[1], &[(A, 49)]));
    assert_eq!(kind_of(&one_coin, 9), Some((ActivityKind::ToSelf, 49)), "one coin is not joined to anything");
    let mixed = shape(&pay(&[1, 9], &[(A, 99)]));
    assert_eq!(kind_of(&mixed, 9), Some((ActivityKind::ToSelf, 99)), "coins of two addresses");
}

#[test]
fn coins_kept_in_the_wallet_are_not_sent() {
    let sent = shape(&pay(&[1], &[(STRANGER, 30), (A, 19)]));
    assert_eq!(kind_of(&sent, 5), Some((ActivityKind::Sent, 30)));
    assert_eq!(sent.fee(), Some(1));
    let received = shape(&pay(&[20], &[(A, 50), (STRANGER, 10)]));
    assert_eq!(kind_of(&received, 5), Some((ActivityKind::Received, 50)));
    assert_eq!(received.fee(), None, "the coins it spent are someone else's");
    assert_eq!(kind_of(&shape(&pay(&[20], &[(STRANGER, 50)])), 5), None);
    let mut mined = pay(&[], &[(A, 50)]);
    mined.coinbase_height = Some(5);
    assert_eq!(kind_of(&shape(&mined), 5), Some((ActivityKind::Mined, 50)));
}
