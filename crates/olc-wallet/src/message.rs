//! Signed messages, as Bitcoin's signmessage makes them: the text behind a prefix of OLC's own,
//! hashed twice, and a signature its key can be recovered from. Anyone holding the message, the
//! signature and the address checks it with no chain at all.
use crate::Wallet;
use alloc::string::String;
use alloc::vec::Vec;
use base64ct::{Base64, Encoding};
use k256::ecdsa::{RecoveryId, Signature, SigningKey, VerifyingKey};
use olc_core::Address;
use olc_crypto::{address_of, hash, PublicKey};

/// Bitcoin's is "Bitcoin Signed Message:\n". A different one keeps a signature from passing as
/// a payment's or as another coin's.
pub const MESSAGE_PREFIX: &str = "OfflineCoin Signed Message:\n";
/// Bitcoin's first byte for a compressed key: 27, plus 4 for compressed, plus the recovery id.
const COMPRESSED: u8 = 31;

fn with_length(out: &mut Vec<u8>, text: &[u8]) {
    // Bitcoin's compact size: one byte below 253, then a marker and the length.
    match u16::try_from(text.len()) {
        Ok(short) if short < 253 => out.push(short as u8),
        Ok(short) => {
            out.push(253);
            out.extend_from_slice(&short.to_le_bytes());
        }
        Err(_) => {
            out.push(254);
            out.extend_from_slice(&(text.len() as u32).to_le_bytes());
        }
    }
    out.extend_from_slice(text);
}

/// What a message signature covers.
pub fn message_hash(message: &str) -> [u8; 32] {
    let mut bytes = Vec::with_capacity(message.len() + 40);
    with_length(&mut bytes, MESSAGE_PREFIX.as_bytes());
    with_length(&mut bytes, message.as_bytes());
    hash(&bytes)
}

impl Wallet {
    /// Signs `message` with the key of `address`, as base64 text. `None` when this wallet does
    /// not hold that key.
    pub fn sign_message(&self, address: &Address, message: &str) -> Option<String> {
        let key = self.keys.iter().find(|key| key.address == *address)?;
        let signing = SigningKey::from_bytes(&(*key.secret().ok()?).into()).ok()?;
        let (signature, recovery) = signing.sign_prehash_recoverable(&message_hash(message)).ok()?;
        let mut bytes = Vec::with_capacity(65);
        bytes.push(COMPRESSED + recovery.to_byte());
        bytes.extend_from_slice(&signature.to_bytes());
        Some(Base64::encode_string(&bytes))
    }
}

/// True when `signature` is the key of `address` signing `message`.
pub fn verify_message(address: &Address, message: &str, signature: &str) -> bool {
    let Ok(bytes) = Base64::decode_vec(signature.trim()) else {
        return false;
    };
    let Some((&first, rest)) = bytes.split_first() else {
        return false;
    };
    let recovery = first.checked_sub(COMPRESSED).and_then(RecoveryId::from_byte);
    let (Some(recovery), Ok(signature)) = (recovery, Signature::from_slice(rest)) else {
        return false;
    };
    let Ok(key) = VerifyingKey::recover_from_prehash(&message_hash(message), &signature, recovery) else {
        return false;
    };
    let point = key.to_encoded_point(true);
    let public: Option<PublicKey> = point.as_bytes().try_into().ok();
    public.is_some_and(|public| address_of(&public) == *address)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_signed_message_checks_against_its_address_only() {
        let wallet = Wallet::from_secret([5; 32]).unwrap();
        let other = Wallet::from_secret([6; 32]).unwrap();
        let address = wallet.address();
        let signature = wallet.sign_message(&address, "I own this address").unwrap();
        assert!(verify_message(&address, "I own this address", &signature));
        assert!(!verify_message(&address, "I own that address", &signature));
        assert!(!verify_message(&other.address(), "I own this address", &signature));
        assert!(!verify_message(&address, "I own this address", "not a signature"));
        assert_eq!(other.sign_message(&address, "x"), None);
    }

    #[test]
    fn lengths_are_written_as_bitcoin_writes_them() {
        // Bitcoin's own prefix gives Bitcoin's hash: the shape is the same, only the words differ.
        let mut bytes = Vec::new();
        with_length(&mut bytes, MESSAGE_PREFIX.as_bytes());
        assert_eq!(bytes[0] as usize, MESSAGE_PREFIX.len());
        let long = "x".repeat(300);
        let mut out = Vec::new();
        with_length(&mut out, long.as_bytes());
        assert_eq!(&out[..3], &[253, 44, 1]);
    }
}
