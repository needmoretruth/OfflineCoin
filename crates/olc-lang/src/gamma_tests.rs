use super::op::*;
use super::*;
use alloc::vec;
use olc_core::sighash::{ALL, ANYONE_CAN_PAY, SINGLE};
use olc_core::{OutPoint, Tx, TxIn, TxOut};
use olc_crypto::{public_key, sign};

const COIN: [u8; 32] = [5; 32];
const LOCK: &[u8] = b"the lock";

fn tx(version: u32, sequence: u32) -> Tx {
    let previous = OutPoint { txid: [1; 32], index: 0 };
    Tx {
        version,
        inputs: vec![TxIn { previous, unlock: Vec::new(), sequence }],
        outputs: vec![TxOut { value: 9, language: 0, lock: vec![2; 20] }],
        coinbase_height: None,
        lock_time: 0,
    }
}

fn context(tx: &Tx, height: u64) -> SpendContext<'_> {
    let spending = Spending { tx, input: 0, lock: LOCK, coin: COIN };
    SpendContext { signature_hash: tx.signature_hash(0, LOCK, Some(&COIN)), height, spending: Some(spending) }
}

/// The stack after running `script` alone, or `None` when it fails.
fn stack_after(script: &[u8]) -> Option<Vec<Vec<u8>>> {
    let tx = tx(2, 0);
    let context = context(&tx, 100);
    let spending = context.spending?;
    let mut machine = Machine { stack: Vec::new(), alt: Vec::new(), ops: 0, context: &context, spending };
    machine.run(script).map(|()| machine.stack)
}

fn n(value: i64) -> u8 {
    u8::try_from(value).map_or(NEGATIVE_ONE, |small| ONE + small - 1)
}

fn numbers(values: &[i64]) -> Vec<Vec<u8>> {
    values.iter().map(|value| number_bytes(*value)).collect()
}

fn runs_to(script: &[u8], expected: &[i64]) {
    assert_eq!(stack_after(script), Some(numbers(expected)), "{script:02x?}");
}

#[test]
fn script_numbers_are_bitcoins() {
    for (value, bytes) in [(0, vec![]), (1, vec![1]), (-1, vec![0x81]), (127, vec![0x7f]), (128, vec![0x80, 0]),
        (-128, vec![0x80, 0x80]), (255, vec![0xff, 0]), (32_768, vec![0, 0x80, 0])] {
        assert_eq!(number_bytes(value), bytes, "{value}");
        assert_eq!(read_number(&bytes, 4), Some(value));
    }
    assert_eq!(read_number(&[1, 2, 3, 4, 5], 4), None, "five bytes do not go into arithmetic");
    assert!(!is_true(&[]) && !is_true(&[0, 0]) && !is_true(&[0, 0x80]) && is_true(&[0x80, 0]) && is_true(&[1]));
}

#[test]
fn stack_words_move_values_as_in_bitcoin() {
    runs_to(&[n(1), n(2), n(3), ROT], &[2, 3, 1]);
    runs_to(&[n(1), n(2), TUCK], &[2, 1, 2]);
    runs_to(&[n(1), n(2), NIP], &[2]);
    runs_to(&[n(1), n(2), OVER], &[1, 2, 1]);
    runs_to(&[n(1), n(2), n(3), n(2), PICK], &[1, 2, 3, 1]);
    runs_to(&[n(1), n(2), n(3), n(2), ROLL], &[2, 3, 1]);
    runs_to(&[n(1), n(2), n(3), n(4), SWAP_2], &[3, 4, 1, 2]);
    runs_to(&[n(1), n(2), n(3), n(4), OVER_2], &[1, 2, 3, 4, 1, 2]);
    runs_to(&[n(1), n(2), n(3), n(4), n(5), n(6), ROT_2], &[3, 4, 5, 6, 1, 2]);
    runs_to(&[n(1), n(2), DUP_2], &[1, 2, 1, 2]);
    runs_to(&[n(1), n(2), n(3), DUP_3], &[1, 2, 3, 1, 2, 3]);
    runs_to(&[n(1), n(2), DROP_2], &[]);
    runs_to(&[ZERO, IF_DUP], &[0]);
    runs_to(&[n(1), IF_DUP], &[1, 1]);
    runs_to(&[n(1), n(2), DEPTH], &[1, 2, 2]);
    runs_to(&[n(1), n(2), TO_ALT, n(3), FROM_ALT], &[1, 3, 2]);
    runs_to(&[ZERO, NOT_IF, n(5), ELSE, n(6), END_IF], &[5]);
    runs_to(&[n(1), NOT_IF, n(5), ELSE, n(6), END_IF], &[6]);
    assert_eq!(stack_after(&[3, b'a', b'b', b'c', SIZE]), Some(vec![b"abc".to_vec(), vec![3]]));
    assert_eq!(stack_after(&[FROM_ALT]), None, "nothing on the other stack");
}

#[test]
fn number_words_count_as_in_bitcoin() {
    runs_to(&[n(5), ADD_1], &[6]);
    runs_to(&[n(5), SUB_1], &[4]);
    runs_to(&[n(5), NEGATE], &[-5]);
    runs_to(&[n(5), NEGATE, ABS], &[5]);
    runs_to(&[ZERO, NOT, n(7), NOT, n(7), NOT_ZERO], &[1, 0, 1]);
    runs_to(&[n(7), n(3), SUB, n(3), n(7), SUB], &[4, -4]);
    runs_to(&[n(1), ZERO, BOOL_AND, n(1), ZERO, BOOL_OR], &[0, 1]);
    runs_to(&[n(3), n(3), NUM_EQUAL, n(3), n(4), NUM_NOT_EQUAL], &[1, 1]);
    runs_to(&[n(3), n(4), LESS, n(3), n(4), GREATER], &[1, 0]);
    runs_to(&[n(4), n(4), LESS_OR_EQUAL, n(3), n(4), GREATER_OR_EQUAL], &[1, 0]);
    runs_to(&[n(3), n(4), MIN, n(3), n(4), MAX], &[3, 4]);
    runs_to(&[n(3), n(2), n(5), WITHIN, n(5), n(2), n(5), WITHIN], &[1, 0]);
    runs_to(&[n(3), n(3), NUM_EQUAL_VERIFY], &[]);
    assert_eq!(stack_after(&[n(3), n(4), NUM_EQUAL_VERIFY]), None);
    assert_eq!(stack_after(&[5, 1, 2, 3, 4, 5, ADD_1]), None, "five bytes do not go into arithmetic");
    runs_to(&[n(-1), n(16), ADD], &[15]);
}

#[test]
fn hash_words_give_the_known_digests() {
    let after = |code: u8| stack_after(&[3, b'a', b'b', b'c', code]).unwrap().remove(0);
    let hex = |bytes: Vec<u8>| bytes.iter().map(|byte| alloc::format!("{byte:02x}")).collect::<alloc::string::String>();
    assert_eq!(hex(after(SHA256)), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    assert_eq!(hex(after(RIPEMD160)), "8eb208f7e05d987a9b044a8e98c6b087f15a0bfc");
    assert_eq!(after(HASH160), ripemd160(&sha256(b"abc")).to_vec());
    assert_eq!(after(HASH256), hash(b"abc").to_vec());
    assert_eq!(after(HASH), hash(b"abc")[..20].to_vec());
}

#[test]
fn a_word_bitcoin_turned_off_fails_even_where_it_does_not_run() {
    assert_eq!(stack_after(&[ZERO, IF, 0x7e, END_IF, n(1)]), None);
    assert_eq!(stack_after(&[ZERO, IF, 0xab, END_IF, n(1)]), Some(numbers(&[1])), "unknown words wait to run");
    assert_eq!(stack_after(&[0xab]), None, "CODESEPARATOR is left out");
}

fn key(seed: u8) -> ([u8; 32], Vec<u8>) {
    let secret = [seed; 32];
    (secret, public_key(&secret).unwrap().to_vec())
}

fn push(script: &mut Vec<u8>, data: &[u8]) {
    script.push(u8::try_from(data.len()).unwrap());
    script.extend_from_slice(data);
}

fn spends(tx: &Tx, lock: &[u8], unlock: &[u8]) -> bool {
    Gamma.check(lock, unlock, &context(tx, 100))
}

#[test]
fn a_signature_may_name_its_kind() {
    let tx = tx(2, 0);
    let (secret, public) = key(3);
    let mut lock = Vec::new();
    push(&mut lock, &public);
    lock.push(CHECK_SIG);
    let plain = sign(&secret, &tx.signature_hash(0, LOCK, Some(&COIN))).unwrap().to_vec();
    let mut unlock = Vec::new();
    push(&mut unlock, &plain);
    assert!(spends(&tx, &lock, &unlock), "64 bytes sign it all");

    let kind = SINGLE | ANYONE_CAN_PAY;
    let digest = tx.signature_hash_as(0, LOCK, &COIN, kind).unwrap();
    let mut typed = sign(&secret, &digest).unwrap().to_vec();
    typed.push(kind);
    let mut more = tx.clone();
    more.inputs.push(TxIn { previous: OutPoint { txid: [8; 32], index: 1 }, unlock: Vec::new(), sequence: 0 });
    more.outputs.push(TxOut { value: 1, language: 0, lock: vec![3; 20] });
    let mut unlock = Vec::new();
    push(&mut unlock, &typed);
    assert!(spends(&more, &lock, &unlock), "others added an input and an output");
    *unlock.last_mut().unwrap() = ALL;
    assert!(!spends(&more, &lock, &unlock), "the kind byte is signed");

    let before = SpendContext { signature_hash: tx.signature_hash(0, LOCK, None), height: 100, spending: None };
    assert!(!Gamma.check(&lock, &unlock, &before), "nothing is spent from gamma before the upgrade");
}

#[test]
fn a_multi_signature_takes_up_to_twenty_keys() {
    let tx = tx(2, 0);
    let digest = tx.signature_hash(0, LOCK, Some(&COIN));
    let lock_of = |count: u8| {
        let mut lock = vec![1, count];
        for seed in 1..=count {
            push(&mut lock, &key(seed).1);
        }
        lock.extend([1, count, CHECK_MULTI_SIG]);
        lock
    };
    let unlock_of = |count: u8| {
        let mut unlock = Vec::new();
        for seed in 1..=count {
            push(&mut unlock, &sign(&key(seed).0, &digest).unwrap());
        }
        unlock
    };
    assert!(spends(&tx, &lock_of(20), &unlock_of(20)), "20 of 20");
    assert!(!spends(&tx, &lock_of(21), &unlock_of(21)), "21 keys are too many");
    assert_eq!(signature_checks(&lock_of(20)), 20);
    let mut two_of_three = vec![n(2)];
    for seed in 1..=3 {
        push(&mut two_of_three, &key(seed).1);
    }
    two_of_three.extend([n(3), CHECK_MULTI_SIG_VERIFY, n(1)]);
    assert_eq!(signature_checks(&two_of_three), 3, "a small number word gives the count");
    let mut unlock = Vec::new();
    push(&mut unlock, &sign(&key(1).0, &digest).unwrap());
    push(&mut unlock, &sign(&key(3).0, &digest).unwrap());
    assert!(spends(&tx, &two_of_three, &unlock));
}

#[test]
fn checksequenceverify_reads_the_inputs_sequence() {
    let lock = [n(3), CHECK_SEQUENCE, DROP, n(1)];
    assert!(spends(&tx(2, 3), &lock, &[]));
    assert!(!spends(&tx(2, 2), &lock, &[]), "the input locks too little");
    assert!(!spends(&tx(1, 3), &lock, &[]), "an old transaction has no sequence");
    assert!(!spends(&tx(2, SEQUENCE_DISABLE | 3), &lock, &[]));
    assert!(!spends(&tx(2, SEQUENCE_BY_TIME | 3), &lock, &[]), "time is not counted");
    let mut off = vec![5];
    off.extend(number_bytes(i64::from(SEQUENCE_DISABLE)));
    off.extend([CHECK_SEQUENCE, DROP, n(1)]);
    assert!(spends(&tx(2, 0), &off, &[]), "a value with the off bit asks nothing");
}
