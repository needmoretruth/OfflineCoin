//! Lock: a 20-byte address. Unlock: a public key followed by a signature.
use crate::ALPHA;
use alloc::vec::Vec;
use olc_core::{LockLanguage, SpendContext};
use olc_crypto::{address_of, verify, Address, PublicKey, SignatureBytes};

pub struct Alpha;

pub const UNLOCK_LEN: usize = 33 + 64;
/// An unlock whose signature names its kind. See [`olc_core::sighash`].
pub const TYPED_LEN: usize = UNLOCK_LEN + 1;

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
        // From the upgrade a 98th byte names the signature's kind, as in Bitcoin.
        let digest = match (unlock.len(), context.spending) {
            (UNLOCK_LEN, _) => Some(context.signature_hash),
            (TYPED_LEN, Some(spending)) => spending.signature_hash(unlock[UNLOCK_LEN]),
            _ => None,
        };
        let Some(digest) = digest else {
            return false;
        };
        let (public_key, signature) = unlock[..UNLOCK_LEN].split_at(33);
        let Ok(public_key) = <&PublicKey>::try_from(public_key) else {
            return false;
        };
        address_of(public_key) == *address && verify(public_key, &digest, signature)
    }
}
