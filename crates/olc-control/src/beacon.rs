//! A beacon is signed with the key behind the board's own address, so one relayed over radio
//! cannot speak for a board that did not send it. It says nothing secret and gives no control.
use crate::packet::{Beacon, BoardId};
use alloc::vec::Vec;
use olc_crypto::{address_of, hash, public_key, sign, verify, Hash, PublicKey};
use serde::{Deserialize, Serialize};

const DOMAIN: &[u8] = b"olc board beacon";

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedBeacon {
    pub beacon: Beacon,
    pub public_key: Vec<u8>,
    pub signature: Vec<u8>,
}

fn digest(board: &BoardId, beacon: &Beacon) -> Hash {
    let mut bytes = Vec::from(DOMAIN);
    bytes.extend_from_slice(board);
    bytes.extend(postcard::to_allocvec(beacon).unwrap_or_default());
    hash(&bytes)
}

pub fn sign_beacon(secret: &[u8; 32], board: &BoardId, beacon: Beacon) -> Option<SignedBeacon> {
    Some(SignedBeacon {
        public_key: public_key(secret)?.to_vec(),
        signature: sign(secret, &digest(board, &beacon))?.to_vec(),
        beacon,
    })
}

impl SignedBeacon {
    /// True when the holder of the beacon's address signed it for `board`.
    pub fn is_from(&self, board: &BoardId) -> bool {
        let Ok(key) = PublicKey::try_from(self.public_key.as_slice()) else {
            return false;
        };
        address_of(&key) == self.beacon.address
            && verify(&key, &digest(board, &self.beacon), &self.signature)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: [u8; 32] = [5; 32];
    const BOARD: BoardId = [1; 6];

    fn signed() -> SignedBeacon {
        let key = public_key(&SECRET).unwrap();
        let beacon = Beacon {
            address: address_of(&key),
            height: 7,
            ..Beacon::default()
        };
        sign_beacon(&SECRET, &BOARD, beacon).unwrap()
    }

    #[test]
    fn a_signed_beacon_comes_from_its_board() {
        assert!(signed().is_from(&BOARD));
    }

    #[test]
    fn a_changed_beacon_is_nobodys() {
        let mut changed = signed();
        changed.beacon.height = 8;
        assert!(!changed.is_from(&BOARD));
    }

    #[test]
    fn a_beacon_cannot_move_to_another_board() {
        assert!(!signed().is_from(&[2; 6]));
    }

    #[test]
    fn another_key_cannot_sign_for_an_address() {
        let mut stolen = signed();
        let other = sign_beacon(&[6; 32], &BOARD, stolen.beacon.clone()).unwrap();
        stolen.public_key = other.public_key;
        stolen.signature = other.signature;
        assert!(!stolen.is_from(&BOARD));
    }
}
