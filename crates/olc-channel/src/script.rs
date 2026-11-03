//! The locks of BOLT 3 in gamma, and what opens each. OLC keeps a lock whole in its output rather
//! than behind a hash, so whoever holds a revocation key can tell its outputs by their first words.
use olc_crypto::{ripemd160, sha256, Hash, PublicKey, SignatureBytes};
use olc_lang::gamma::{number_bytes, op};

/// Keys, hashes, signatures and numbers are all this crate pushes, none near 256 bytes.
fn push(script: &mut Vec<u8>, data: &[u8]) {
    let length = u8::try_from(data.len()).unwrap_or(u8::MAX);
    match length {
        0 => script.push(op::ZERO),
        length if length <= op::PUSH_MAX => {
            script.push(length);
            script.extend_from_slice(data);
        }
        length => {
            script.push(op::PUSH_DATA_1);
            script.push(length);
            script.extend_from_slice(&data[..usize::from(length)]);
        }
    }
}

fn push_number(script: &mut Vec<u8>, value: i64) {
    match value {
        0 => script.push(op::ZERO),
        1..=16 => script.push(op::ONE + u8::try_from(value - 1).unwrap_or_default()),
        _ => push(script, &number_bytes(value)),
    }
}

/// HASH160 of a key, as the revocation branch compares it.
fn key_hash(key: &PublicKey) -> [u8; 20] {
    ripemd160(&sha256(key))
}

/// Both keys sign, smaller key first, as BOLT 3's funding output.
pub fn funding(one: &PublicKey, other: &PublicKey) -> Vec<u8> {
    let (first, second) = if one <= other { (one, other) } else { (other, one) };
    let mut script = Vec::new();
    push_number(&mut script, 2);
    push(&mut script, first);
    push(&mut script, second);
    push_number(&mut script, 2);
    script.push(op::CHECK_MULTI_SIG);
    script
}

/// Opens [`funding`] with a signature for each key. OLC's multi-signature takes no extra value.
pub fn funding_unlock(one: (&PublicKey, &SignatureBytes), other: (&PublicKey, &SignatureBytes)) -> Vec<u8> {
    let (first, second) = if one.0 <= other.0 { (one, other) } else { (other, one) };
    let mut unlock = Vec::new();
    push(&mut unlock, first.1);
    push(&mut unlock, second.1);
    unlock
}

/// The owner's own coins: the other side takes them with the revocation key at once, the owner
/// after `delay` blocks (BOLT 3, to_local).
pub fn to_self(revocation: &PublicKey, delay: u16, delayed: &PublicKey) -> Vec<u8> {
    let mut script = vec![op::IF];
    push(&mut script, revocation);
    script.push(op::ELSE);
    push_number(&mut script, i64::from(delay));
    script.extend_from_slice(&[op::CHECK_SEQUENCE, op::DROP]);
    push(&mut script, delayed);
    script.extend_from_slice(&[op::END_IF, op::CHECK_SIG]);
    script
}

/// The start of a [`to_self`] lock under `revocation`: whatever follows, that key opens it.
pub fn to_self_start(revocation: &PublicKey) -> Vec<u8> {
    let mut start = vec![op::IF];
    push(&mut start, revocation);
    start.push(op::ELSE);
    start
}

pub fn to_self_by_revocation(signature: &SignatureBytes) -> Vec<u8> {
    let mut unlock = Vec::new();
    push(&mut unlock, signature);
    unlock.push(op::ONE);
    unlock
}

/// The owner's way, in an input whose sequence waits the delay.
pub fn to_self_after_delay(signature: &SignatureBytes) -> Vec<u8> {
    let mut unlock = Vec::new();
    push(&mut unlock, signature);
    unlock.push(op::ZERO);
    unlock
}

/// Keys of one HTLC output.
pub struct HtlcKeys<'a> {
    pub revocation: &'a PublicKey,
    /// The key of the side that does not hold this commitment.
    pub remote: &'a PublicKey,
    /// The key of the side that holds it.
    pub local: &'a PublicKey,
}

fn revocation_branch(keys: &HtlcKeys) -> Vec<u8> {
    let mut script = vec![op::DUP, op::HASH160];
    push(&mut script, &key_hash(keys.revocation));
    script.extend_from_slice(&[op::EQUAL, op::IF, op::CHECK_SIG, op::ELSE]);
    push(&mut script, keys.remote);
    script.extend_from_slice(&[op::SWAP, op::SIZE]);
    push_number(&mut script, 32);
    script.push(op::EQUAL);
    script
}

/// The start every HTLC output under `revocation` has.
pub fn htlc_start(revocation: &PublicKey) -> Vec<u8> {
    let mut start = vec![op::DUP, op::HASH160];
    push(&mut start, &key_hash(revocation));
    start.extend_from_slice(&[op::EQUAL, op::IF, op::CHECK_SIG]);
    start
}

fn two_of_two_tail(script: &mut Vec<u8>, local: &PublicKey) {
    push_number(script, 2);
    script.push(op::SWAP);
    push(script, local);
    push_number(script, 2);
    script.push(op::CHECK_MULTI_SIG);
}

/// A payment the holder offered: the other side takes it with the preimage, the holder takes it
/// back through an HTLC-timeout transaction both signed (BOLT 3, offered HTLC).
pub fn offered_htlc(keys: &HtlcKeys, payment_hash: &Hash) -> Vec<u8> {
    let mut script = revocation_branch(keys);
    script.extend_from_slice(&[op::NOT_IF, op::DROP]);
    two_of_two_tail(&mut script, keys.local);
    script.extend_from_slice(&[op::ELSE, op::HASH160]);
    push(&mut script, &ripemd160(payment_hash));
    script.extend_from_slice(&[op::EQUAL_VERIFY, op::CHECK_SIG, op::END_IF, op::END_IF]);
    script
}

/// A payment the holder is offered: the holder takes it with the preimage through an HTLC-success
/// transaction, the other side takes it back from `expiry` (BOLT 3, received HTLC).
pub fn received_htlc(keys: &HtlcKeys, payment_hash: &Hash, expiry: u32) -> Vec<u8> {
    let mut script = revocation_branch(keys);
    script.extend_from_slice(&[op::IF, op::HASH160]);
    push(&mut script, &ripemd160(payment_hash));
    script.push(op::EQUAL_VERIFY);
    two_of_two_tail(&mut script, keys.local);
    script.extend_from_slice(&[op::ELSE, op::DROP]);
    push_number(&mut script, i64::from(expiry));
    script.extend_from_slice(&[op::CHECK_HEIGHT, op::DROP, op::CHECK_SIG, op::END_IF, op::END_IF]);
    script
}

/// Opens either HTLC with the revocation key.
pub fn htlc_by_revocation(signature: &SignatureBytes, revocation: &PublicKey) -> Vec<u8> {
    let mut unlock = Vec::new();
    push(&mut unlock, signature);
    push(&mut unlock, revocation);
    unlock
}

/// The holder's second-stage transaction, signed by both: HTLC-timeout with no preimage,
/// HTLC-success with one.
pub fn htlc_by_both(remote: &SignatureBytes, local: &SignatureBytes, preimage: Option<&Hash>) -> Vec<u8> {
    let mut unlock = Vec::new();
    push(&mut unlock, remote);
    push(&mut unlock, local);
    push(&mut unlock, preimage.map_or(&[][..], |preimage| &preimage[..]));
    unlock
}

/// The other side's own way: the preimage for an offered HTLC, nothing after the expiry of a
/// received one.
pub fn htlc_by_remote(signature: &SignatureBytes, preimage: Option<&Hash>) -> Vec<u8> {
    let mut unlock = Vec::new();
    push(&mut unlock, signature);
    push(&mut unlock, preimage.map_or(&[][..], |preimage| &preimage[..]));
    unlock
}
