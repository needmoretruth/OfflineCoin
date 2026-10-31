//! Lock: a 20-byte address. Unlock: a public key followed by a signature.
use crate::ALPHA;
use alloc::vec::Vec;
use olc_core::{LockLanguage, SpendContext};
use olc_crypto::{address_of, verify, Address, PublicKey, SignatureBytes};

pub struct Alpha;

pub const UNLOCK_LEN: usize = 33 + 64;

pub fn unlock_data(public_key: &PublicKey, signature: &SignatureBytes) -> Vec<u8> {
    let mut data = Vec::with_capacity(UNLOCK_LEN);
    data.extend_from_slice(public_key);
    data.extend_from_slice(signature);
    data
}

impl LockLanguage for Alpha {
    fn id(&self) -> u8 {
        ALPHA
    }

    fn name(&self) -> &'static str {
        "alpha"
    }

    fn check(&self, lock: &[u8], unlock: &[u8], context: &SpendContext) -> bool {
        let Ok(address) = <&Address>::try_from(lock) else {
            return false;
        };
        if unlock.len() != UNLOCK_LEN {
            return false;
        }
        let (public_key, signature) = unlock.split_at(33);
        let Ok(public_key) = <&PublicKey>::try_from(public_key) else {
            return false;
        };
        address_of(public_key) == *address && verify(public_key, &context.signature_hash, signature)
    }
}
