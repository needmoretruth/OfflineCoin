//! Keys of a channel, as BOLT 3 makes them. Each side holds base keys for the whole channel and a
//! secret per state from one seed; every key in a commitment is a base key moved by that state's
//! point, so revealing one state's secret gives the other side the key to that state alone.
use crate::bytes::{Bytes, Key};
use olc_crypto::tweak::{add_to_public, add_to_secret, sum_of_publics, sum_of_secrets};
use olc_crypto::{public_key, sha256, Hash, PublicKey};
use serde::{Deserialize, Serialize};

/// The highest index a per-state secret has. State `n` takes index `LAST_INDEX - n`.
pub const LAST_INDEX: u64 = (1 << 48) - 1;

/// The secret of a state, from a seed (BOLT 3, `generate_from_seed`).
pub fn secret_at_index(seed: &Hash, index: u64) -> Hash {
    derive_down(*seed, 48, index)
}

/// The per-state secret of state `number`.
pub fn state_secret(seed: &Hash, number: u64) -> Hash {
    secret_at_index(seed, LAST_INDEX - number)
}

/// The point the other side moves its keys by for state `number`.
pub fn state_point(seed: &Hash, number: u64) -> Option<PublicKey> {
    public_key(&state_secret(seed, number))
}

fn derive_down(mut value: Hash, bits: u32, index: u64) -> Hash {
    for bit in (0..bits).rev() {
        if index >> bit & 1 == 1 {
            // `bits` is at most 48, so the byte is one of the first six.
            value[usize::try_from(bit / 8).unwrap_or_default()] ^= 1 << (bit % 8);
            value = sha256(&value);
        }
    }
    value
}

/// The other side's revealed secrets in 49 slots, however many states there were (BOLT 3).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretStore {
    known: Vec<(u64, Hash)>,
}

fn slot_of(index: u64) -> usize {
    (0..48).find(|bit| index >> bit & 1 == 1).unwrap_or(48)
}

impl SecretStore {
    /// Takes the secret of state `number`. Refuses one that does not fit those before it, which
    /// would mean the other side is not deriving its secrets from one seed.
    pub fn insert(&mut self, number: u64, secret: Hash) -> bool {
        let index = LAST_INDEX - number;
        let slot = slot_of(index);
        for (known_index, known) in self.known.iter().take(slot) {
            if derive_down(secret, u32::try_from(slot).unwrap_or(u32::MAX), *known_index) != *known {
                return false;
            }
        }
        // Secrets come in order, so a gap means one was skipped.
        if self.known.len() < slot {
            return false;
        }
        match self.known.get_mut(slot) {
            Some(place) => *place = (index, secret),
            None => self.known.push((index, secret)),
        }
        true
    }

    /// The secret of state `number`, if it was revealed.
    pub fn get(&self, number: u64) -> Option<Hash> {
        let index = LAST_INDEX.checked_sub(number)?;
        self.known.iter().enumerate().find_map(|(bits, (known_index, secret))| {
            let mask = !((1u64 << bits) - 1);
            let bits = u32::try_from(bits).unwrap_or(u32::MAX);
            (index & mask == *known_index).then(|| derive_down(*secret, bits, index))
        })
    }
}

/// The public base keys a side shows when the channel opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Basepoints {
    pub funding: Key,
    pub revocation: Key,
    pub delayed: Key,
    pub htlc: Key,
}

/// A side's own secrets, all from one channel seed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Secrets {
    pub funding: [u8; 32],
    pub revocation: [u8; 32],
    pub delayed: [u8; 32],
    pub htlc: [u8; 32],
    pub states: Hash,
}

fn labelled(seed: &Hash, label: &[u8]) -> [u8; 32] {
    let mut data = seed.to_vec();
    data.extend_from_slice(label);
    sha256(&data)
}

impl Secrets {
    pub fn from_seed(seed: &Hash) -> Self {
        Self {
            funding: labelled(seed, b"funding"),
            revocation: labelled(seed, b"revocation"),
            delayed: labelled(seed, b"delayed"),
            htlc: labelled(seed, b"htlc"),
            states: labelled(seed, b"states"),
        }
    }

    pub fn basepoints(&self) -> Option<Basepoints> {
        Some(Basepoints {
            funding: Bytes(public_key(&self.funding)?),
            revocation: Bytes(public_key(&self.revocation)?),
            delayed: Bytes(public_key(&self.delayed)?),
            htlc: Bytes(public_key(&self.htlc)?),
        })
    }
}

fn hash_of_two(first: &[u8], second: &[u8]) -> Hash {
    let mut data = first.to_vec();
    data.extend_from_slice(second);
    sha256(&data)
}

/// `basepoint + SHA256(point || basepoint)·G` (BOLT 3, localpubkey and the others).
pub fn derive_public(basepoint: &PublicKey, point: &PublicKey) -> Option<PublicKey> {
    add_to_public(basepoint, &hash_of_two(point, basepoint))
}

/// The secret of [`derive_public`].
pub fn derive_secret(base_secret: &[u8; 32], point: &PublicKey) -> Option<[u8; 32]> {
    let basepoint = public_key(base_secret)?;
    add_to_secret(base_secret, &hash_of_two(point, &basepoint))
}

/// The revocation key of a state: only whoever holds both the base secret and the state's secret
/// can sign for it (BOLT 3, revocationpubkey).
pub fn revocation_public(basepoint: &PublicKey, point: &PublicKey) -> Option<PublicKey> {
    sum_of_publics(basepoint, &hash_of_two(basepoint, point), point, &hash_of_two(point, basepoint))
}

/// The secret of [`revocation_public`], once the state's secret is out.
pub fn revocation_secret(base_secret: &[u8; 32], state_secret: &Hash) -> Option<[u8; 32]> {
    let basepoint = public_key(base_secret)?;
    let point = public_key(state_secret)?;
    sum_of_secrets(
        base_secret,
        &hash_of_two(&basepoint, &point),
        state_secret,
        &hash_of_two(&point, &basepoint),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(text: &str) -> Vec<u8> {
        (0..text.len()).step_by(2).map(|at| u8::from_str_radix(&text[at..at + 2], 16).unwrap()).collect()
    }

    fn array<const N: usize>(text: &str) -> [u8; N] {
        hex(text).try_into().unwrap()
    }

    /// BOLT 3, appendix D: generation tests.
    #[test]
    fn state_secrets_match_the_bolt_3_vectors() {
        let cases = [
            ("0000000000000000000000000000000000000000000000000000000000000000", LAST_INDEX,
             "02a40c85b6f28da08dfdbe0926c53fab2de6d28c10301f8f7c4073d5e42e3148"),
            ("FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF", LAST_INDEX,
             "7cc854b54e3e0dcdb010d7a3fee464a9687be6e8db3be6854c475621e007a5dc"),
            ("FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF", 0xaaaaaaaaaaa,
             "56f4008fb007ca9acf0e15b054d5c9fd12ee06cea347914ddbaed70d1c13a528"),
            ("FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF", 0x555555555555,
             "9015daaeb06dba4ccc05b91b2f73bd54405f2be9f217fbacd3c5ac2e62327d31"),
            ("0101010101010101010101010101010101010101010101010101010101010101", 1,
             "915c75942a26bb3a433a8ce2cb0427c29ec6c1775cfc78328b57f6ba7bfeaa9c"),
        ];
        for (seed, index, secret) in cases {
            assert_eq!(secret_at_index(&array(seed), index), array::<32>(secret), "{seed} {index}");
        }
    }

    /// BOLT 3, appendix E: key derivation.
    #[test]
    fn derived_keys_match_the_bolt_3_vectors() {
        let base_secret: [u8; 32] = array("000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f");
        let state: Hash = array("1f1e1d1c1b1a191817161514131211100f0e0d0c0b0a09080706050403020100");
        let base_point = public_key(&base_secret).unwrap();
        assert_eq!(base_point, array::<33>("036d6caac248af96f6afa7f904f550253a0f3ef3f5aa2fe6838a95b216691468e2"));
        let point = public_key(&state).unwrap();
        assert_eq!(point, array::<33>("025f7117a78150fe2ef97db7cfc83bd57b2e2c0d0dd25eaf467a4a1c2a45ce1486"));
        assert_eq!(
            derive_public(&base_point, &point).unwrap(),
            array::<33>("0235f2dbfaa89b57ec7b055afe29849ef7ddfeb1cefdb9ebdc43f5494984db29e5")
        );
        assert_eq!(
            derive_secret(&base_secret, &point).unwrap(),
            array::<32>("cbced912d3b21bf196a766651e436aff192362621ce317704ea2f75d87e7be0f")
        );
        assert_eq!(
            revocation_public(&base_point, &point).unwrap(),
            array::<33>("02916e326636d19c33f13e8c0c3a03dd157f332f3e99c317c141dd865eb01f8ff0")
        );
        assert_eq!(
            revocation_secret(&base_secret, &state).unwrap(),
            array::<32>("d09ffff62ddb2297ab000cc85bcb4283fdeb6aa052affbc9dddcf33b61078110")
        );
    }

    #[test]
    fn the_store_gives_back_every_secret_from_49_slots() {
        let seed = sha256(b"their seed");
        let mut store = SecretStore::default();
        for number in 0..1_000 {
            assert!(store.insert(number, state_secret(&seed, number)), "{number}");
            assert!(store.known.len() <= 49);
        }
        for number in [0, 1, 2, 500, 999] {
            assert_eq!(store.get(number), Some(state_secret(&seed, number)));
        }
        assert_eq!(store.get(1_000), None, "a state not yet revoked");
        let mut wrong = store.clone();
        // A leaf secret is checked once the one above it arrives, as in BOLT 3.
        assert!(wrong.insert(1_000, state_secret(&seed, 1_000)));
        assert!(!wrong.insert(1_001, sha256(b"made up")), "a secret off the seed");
    }
}
