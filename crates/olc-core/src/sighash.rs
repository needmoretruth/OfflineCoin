//! What a signature covers. Before a coin's upgrade there is one kind, the whole transaction
//! without its unlock data. From the upgrade a signature also names the coin, and it may carry
//! one of Bitcoin's kinds in a last byte: ALL, NONE, SINGLE, each alone or with ANYONECANPAY, which
//! cover what they cover in Bitcoin (BIP 143 lists it). A signature without that byte is ALL.
//!
//! The digest hashes a copy of the transaction cut down to what the kind covers, as Bitcoin's
//! first signature hash did. Its cost grows with the transaction for every signature, which BIP
//! 143 fixed for big transactions; a node here takes at most 20 checks in one transaction, and a
//! block at most a few kilobytes, so the copy stays cheap and the rule stays short.
use crate::types::{Tx, TxOut, COIN_SIGNATURE_START};
use alloc::vec;
use alloc::vec::Vec;
use olc_crypto::{hash, Hash};

/// Covers every input and every output.
pub const ALL: u8 = 0x01;
/// Covers the inputs and no output, so whoever holds it may send the coins anywhere.
pub const NONE: u8 = 0x02;
/// Covers the inputs and the one output at the signed input's place.
pub const SINGLE: u8 = 0x03;
/// Added to one of the three: covers only the signed input, so others may add theirs.
pub const ANYONE_CAN_PAY: u8 = 0x80;

/// Is this one of the six kinds? Any other byte makes the signature fail, so a later rule may
/// give it a meaning.
pub fn is_kind(kind: u8) -> bool {
    matches!(kind & !ANYONE_CAN_PAY, ALL | NONE | SINGLE)
}

impl Tx {
    /// What a signature of kind ALL covers, with `coin` from
    /// [`crate::CoinConfig::signing_id_at`]. As with Bitcoin Cash's fork id, a payment signed for
    /// one coin then fails on every other, a fork and the coin it left included.
    pub fn signature_hash(&self, input: usize, lock: &[u8], coin: Option<&Hash>) -> Hash {
        match coin {
            Some(coin) => self.signature_hash_as(input, lock, coin, ALL).unwrap_or_default(),
            None => self.first_signature_hash(input, lock),
        }
    }

    /// The digest every coin used before its upgrade. Unchanged, so every signature on a chain
    /// from before stays good.
    fn first_signature_hash(&self, input: usize, lock: &[u8]) -> Hash {
        let mut data = self.without_unlocks().encode();
        data.extend_from_slice(&(input as u32).to_le_bytes());
        data.extend_from_slice(lock);
        hash(&data)
    }

    /// What a signature of `kind` covers on an upgraded coin. `None` for a byte that is no kind,
    /// for an input the transaction does not have, and for SINGLE with no output at its place
    /// (where Bitcoin signs the number 1 by mistake).
    pub fn signature_hash_as(&self, input: usize, lock: &[u8], coin: &Hash, kind: u8) -> Option<Hash> {
        if !is_kind(kind) || input >= self.inputs.len() {
            return None;
        }
        let mut copy = self.without_unlocks();
        let base = kind & !ANYONE_CAN_PAY;
        if base != ALL {
            // The other inputs may still change their sequence, as in Bitcoin.
            for (at, other) in copy.inputs.iter_mut().enumerate() {
                if at != input {
                    other.sequence = 0;
                }
            }
        }
        match base {
            NONE => copy.outputs.clear(),
            SINGLE => {
                if input >= copy.outputs.len() {
                    return None;
                }
                copy.outputs.truncate(input + 1);
                let blank = TxOut { value: u64::MAX, language: 0, lock: Vec::new() };
                for earlier in copy.outputs.iter_mut().take(input) {
                    *earlier = blank.clone();
                }
            }
            _ => {}
        }
        let mut place = input;
        if kind & ANYONE_CAN_PAY != 0 {
            copy.inputs = vec![copy.inputs[input].clone()];
            place = 0;
        }
        let mut data = Vec::new();
        data.extend_from_slice(&COIN_SIGNATURE_START);
        data.extend_from_slice(coin);
        data.push(kind);
        data.extend(copy.encode());
        data.extend_from_slice(&(place as u32).to_le_bytes());
        data.extend_from_slice(lock);
        Some(hash(&data))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{OutPoint, TxIn, SEQUENCE_FINAL};

    fn tx(inputs: u8, outputs: u8) -> Tx {
        let input = |at: u8| TxIn {
            previous: OutPoint { txid: [at; 32], index: 0 },
            unlock: vec![at],
            sequence: SEQUENCE_FINAL,
        };
        let output = |at: u8| TxOut { value: u64::from(at) + 1, language: 0, lock: vec![at; 20] };
        Tx {
            version: 2,
            inputs: (0..inputs).map(input).collect(),
            outputs: (0..outputs).map(output).collect(),
            coinbase_height: None,
            lock_time: 0,
        }
    }

    const COIN: Hash = [9; 32];

    fn digest(tx: &Tx, input: usize, kind: u8) -> Option<Hash> {
        tx.signature_hash_as(input, b"lock", &COIN, kind)
    }

    #[test]
    fn each_kind_covers_what_it_covers_in_bitcoin() {
        let base = tx(2, 2);
        let mut more_output = base.clone();
        more_output.outputs.push(TxOut { value: 50, language: 0, lock: vec![] });
        let mut other_output = base.clone();
        other_output.outputs[1].value = 99;
        let mut more_input = base.clone();
        more_input.inputs.push(tx(3, 0).inputs[2].clone());
        let mut other_sequence = base.clone();
        other_sequence.inputs[1].sequence = 5;

        let same = |changed: &Tx, kind: u8| digest(&base, 0, kind) == digest(changed, 0, kind);
        assert!(!same(&more_output, ALL) && !same(&more_input, ALL) && !same(&other_sequence, ALL));
        assert!(same(&more_output, NONE) && same(&other_output, NONE) && same(&other_sequence, NONE));
        assert!(!same(&more_input, NONE));
        assert!(same(&more_output, SINGLE) && same(&other_output, SINGLE) && !same(&more_input, SINGLE));
        assert!(same(&more_input, ALL | ANYONE_CAN_PAY) && !same(&more_output, ALL | ANYONE_CAN_PAY));
        assert!(same(&more_input, SINGLE | ANYONE_CAN_PAY) && same(&more_output, SINGLE | ANYONE_CAN_PAY));
    }

    #[test]
    fn kinds_differ_and_unknown_bytes_sign_nothing() {
        let tx = tx(2, 1);
        let kinds = [ALL, NONE, SINGLE, ALL | ANYONE_CAN_PAY, NONE | ANYONE_CAN_PAY, SINGLE | ANYONE_CAN_PAY];
        let digests: Vec<Hash> = kinds.iter().map(|kind| digest(&tx, 0, *kind).unwrap()).collect();
        for (at, one) in digests.iter().enumerate() {
            assert!(!digests[at + 1..].contains(one), "kind {:#x} has a digest of its own", kinds[at]);
        }
        for kind in [0x00, 0x04, 0x41, 0x84, 0xff] {
            assert_eq!(digest(&tx, 0, kind), None, "{kind:#x}");
        }
        assert_eq!(digest(&tx, 1, SINGLE), None, "no output at the second place");
        assert_eq!(tx.signature_hash(0, b"lock", Some(&COIN)), digest(&tx, 0, ALL).unwrap());
    }
}
