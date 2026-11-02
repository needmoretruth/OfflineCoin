use super::*;
use crate::beta::{unlock_address, Beta};
use olc_core::{LockLanguage, SpendContext};
use olc_crypto::{address_of, hash, public_key, sign};

fn context(height: u64) -> SpendContext<'static> {
    SpendContext { signature_hash: hash(b"spend"), height, spending: None }
}

fn unlocks(lock: &str, unlock: &[u8], height: u64) -> bool {
    let (lock, _) = lock_from_text(lock).unwrap();
    Beta.check(&lock, unlock, &context(height))
}

fn signed(seed: u8) -> ([u8; 33], [u8; 64]) {
    let secret = [seed; 32];
    (public_key(&secret).unwrap(), sign(&secret, &hash(b"spend")).unwrap())
}

fn push(script: &mut Vec<u8>, data: &[u8]) {
    script.push(data.len() as u8);
    script.extend_from_slice(data);
}

#[test]
fn the_hash_lock_opens_for_the_secret_alone() {
    let lock = hash_lock(&hash(b"open sesame")[..20]);
    let (right, _) = assemble(&parse_words("\"open sesame\"").unwrap()).unwrap();
    let (wrong, _) = assemble(&parse_words("\"open barley\"").unwrap()).unwrap();
    assert!(unlocks(&lock, &right, 0));
    assert!(!unlocks(&lock, &wrong, 0));
}

#[test]
fn two_of_two_needs_both_signatures_in_order() {
    let (first, first_signature) = signed(1);
    let (second, second_signature) = signed(2);
    let lock = two_of_two(&first, &second);
    let mut both = Vec::new();
    push(&mut both, &first_signature);
    push(&mut both, &second_signature);
    assert!(unlocks(&lock, &both, 0));
    let mut one = Vec::new();
    push(&mut one, &second_signature);
    assert!(!unlocks(&lock, &one, 0));
    let mut twice = Vec::new();
    push(&mut twice, &first_signature);
    push(&mut twice, &first_signature);
    assert!(!unlocks(&lock, &twice, 0));
}

#[test]
fn the_time_lock_opens_at_its_height() {
    let (key, signature) = signed(3);
    let lock = time_lock(500, &address_of(&key));
    let unlock = unlock_address(&key, &signature);
    assert!(!unlocks(&lock, &unlock, 499));
    assert!(unlocks(&lock, &unlock, 500));
}

#[test]
fn words_read_back_as_they_were_written() {
    for text in [
        "DUP HASH 0x0102030405060708090a0b0c0d0e0f1011121314 EQUALVERIFY CHECKSIG",
        "1 IF \"yes\" ELSE 0 ENDIF DROP",
        "500 CHECKHEIGHT DROP 1",
        "SWAP ADD VERIFY RETURN",
    ] {
        let (bytes, _) = lock_from_text(text).unwrap();
        assert_eq!(words_of(&bytes), text);
    }
}

#[test]
fn lower_case_words_are_the_same_words() {
    assert_eq!(lock_from_text("dup hash").unwrap(), lock_from_text("DUP HASH").unwrap());
}

#[test]
fn a_bad_script_says_what_is_wrong() {
    let wrong = |text: &str| lock_from_text(text).unwrap_err();
    assert_eq!(wrong("DUP SPIN"), WordError::Unknown("SPIN".into()));
    assert_eq!(wrong("0xabc"), WordError::Unknown("0xabc".into()));
    assert_eq!(wrong("\"open"), WordError::OpenQuote);
    assert_eq!(wrong("IF 1"), WordError::OpenBranch);
    assert_eq!(wrong("ENDIF"), WordError::OpenBranch);
    assert_eq!(wrong("  "), WordError::Empty);
    assert_eq!(wrong(&format!("0x{}", "ab".repeat(76))), WordError::TooLong(format!("0x{}", "ab".repeat(76))));
    assert_eq!(wrong(&"DUP ".repeat(201)), WordError::TooManyOps(201));
    assert_eq!(wrong("sig:abc"), WordError::Unknown("sig:abc".into()));
}

#[test]
fn a_script_says_what_it_costs() {
    let (bytes, cost) = lock_from_text("DUP HASH 0x0102 EQUALVERIFY CHECKSIG").unwrap();
    assert_eq!(cost, Cost { bytes: bytes.len(), ops: 5, signature_checks: 1 });
    assert_eq!(bytes.len(), 7);
}

#[test]
fn bytes_that_are_no_script_still_read() {
    assert_eq!(words_of(&[0xff, op::DUP]), "?ff DUP");
    assert_eq!(words_of(&[5, 1]), "?cut");
    assert_eq!(words_of(&[]), "");
}

#[test]
fn an_unlock_keeps_its_signature_place_for_the_wallet() {
    let words = parse_words("sig:abc key:abc \"text\"").unwrap();
    assert_eq!(words[0], Word::SignBy("abc".into()));
    assert_eq!(words[1], Word::KeyOf("abc".into()));
    assert_eq!(words[2], Word::Data(b"text".to_vec()));
    assert_eq!(parse_words(&text_of_words(&words)).unwrap(), words);
}
