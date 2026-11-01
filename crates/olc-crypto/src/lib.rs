//! Hashing and signatures. Every other crate reaches cryptography through this one,
//! so swapping the curve or the hash means changing a single file.
#![no_std]

use k256::ecdsa::signature::hazmat::{PrehashSigner, PrehashVerifier};
use k256::ecdsa::{Signature, SigningKey, VerifyingKey};
use sha2::{Digest, Sha256};

pub type Hash = [u8; 32];
pub type Address = [u8; 20];
pub type PublicKey = [u8; 33];
pub type SignatureBytes = [u8; 64];

/// Bitcoin's hash: SHA-256 applied twice.
pub fn hash(data: &[u8]) -> Hash {
    Sha256::digest(Sha256::digest(data)).into()
}

/// SHA-256 over data that arrives in pieces, such as a firmware image.
#[derive(Clone, Default)]
pub struct StreamHash(Sha256);

impl StreamHash {
    pub fn update(&mut self, data: &[u8]) {
        self.0.update(data);
    }

    pub fn finish(self) -> Hash {
        self.0.finalize().into()
    }
}

/// An address is the first 20 bytes of the hash of a public key.
pub fn address_of(public_key: &PublicKey) -> Address {
    let digest = hash(public_key);
    let mut address = [0u8; 20];
    address.copy_from_slice(&digest[..20]);
    address
}

/// Returns `None` when the 32 bytes are not a valid secp256k1 secret key.
pub fn public_key(secret: &[u8; 32]) -> Option<PublicKey> {
    let key = SigningKey::from_slice(secret).ok()?;
    let point = key.verifying_key().to_encoded_point(true);
    point.as_bytes().try_into().ok()
}

/// Signs a 32-byte digest. The nonce is derived from the key and digest (RFC 6979).
pub fn sign(secret: &[u8; 32], digest: &Hash) -> Option<SignatureBytes> {
    Signer::new(secret)?.sign(digest)
}

/// A parsed key. Parsing costs as much as signing, so something that signs often keeps one.
pub struct Signer {
    key: SigningKey,
    public: PublicKey,
}

impl Signer {
    pub fn new(secret: &[u8; 32]) -> Option<Self> {
        let key = SigningKey::from_slice(secret).ok()?;
        let point = key.verifying_key().to_encoded_point(true);
        let public = point.as_bytes().try_into().ok()?;
        Some(Self { key, public })
    }

    pub fn public_key(&self) -> &PublicKey {
        &self.public
    }

    pub fn sign(&self, digest: &Hash) -> Option<SignatureBytes> {
        let signature: Signature = self.key.sign_prehash(digest).ok()?;
        let signature = signature.normalize_s().unwrap_or(signature);
        Some(signature.to_bytes().into())
    }
}

pub fn verify(public_key: &[u8], digest: &Hash, signature: &[u8]) -> bool {
    let Ok(key) = VerifyingKey::from_sec1_bytes(public_key) else {
        return false;
    };
    let Ok(signature) = Signature::from_slice(signature) else {
        return false;
    };
    key.verify_prehash(digest, &signature).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signature_round_trip() {
        let secret = [7u8; 32];
        let public = public_key(&secret).unwrap();
        let digest = hash(b"message");
        let signature = sign(&secret, &digest).unwrap();
        assert!(verify(&public, &digest, &signature));
        assert!(!verify(&public, &hash(b"other"), &signature));
    }

    #[test]
    fn double_sha256_matches_known_value() {
        // SHA-256d of the empty string.
        assert_eq!(hash(b"")[0..4], [0x5d, 0xf6, 0xe0, 0xe2]);
    }
}
