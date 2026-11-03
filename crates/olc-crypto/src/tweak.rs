//! Keys moved by a public number, as Lightning's BOLT 3 derives a key for each channel state: a
//! base key plus a tweak times the generator, and the revocation key as a sum of two products.
//! Only the curve's own arithmetic is used.
use crate::{Hash, PublicKey};
use k256::elliptic_curve::sec1::{FromEncodedPoint, ToEncodedPoint};
use k256::elliptic_curve::PrimeField;
use k256::{AffinePoint, EncodedPoint, FieldBytes, ProjectivePoint, Scalar};

fn scalar(bytes: &[u8; 32]) -> Option<Scalar> {
    Option::from(Scalar::from_repr(FieldBytes::from(*bytes)))
}

fn point(key: &PublicKey) -> Option<ProjectivePoint> {
    let encoded = EncodedPoint::from_bytes(key).ok()?;
    let affine: Option<AffinePoint> = Option::from(AffinePoint::from_encoded_point(&encoded));
    affine.map(ProjectivePoint::from)
}

fn compressed(point: ProjectivePoint) -> Option<PublicKey> {
    if point == ProjectivePoint::IDENTITY {
        return None;
    }
    point.to_affine().to_encoded_point(true).as_bytes().try_into().ok()
}

fn secret_of(value: Scalar) -> Option<[u8; 32]> {
    if bool::from(value.is_zero()) {
        return None;
    }
    Some(value.to_bytes().into())
}

/// `key + tweak·G`.
pub fn add_to_public(key: &PublicKey, tweak: &Hash) -> Option<PublicKey> {
    compressed(point(key)? + ProjectivePoint::GENERATOR * scalar(tweak)?)
}

/// `secret + tweak`, the secret of [`add_to_public`].
pub fn add_to_secret(secret: &[u8; 32], tweak: &Hash) -> Option<[u8; 32]> {
    secret_of(scalar(secret)? + scalar(tweak)?)
}

/// `first·first_factor + second·second_factor`.
pub fn sum_of_publics(first: &PublicKey, first_factor: &Hash, second: &PublicKey, second_factor: &Hash) -> Option<PublicKey> {
    compressed(point(first)? * scalar(first_factor)? + point(second)? * scalar(second_factor)?)
}

/// The secret of [`sum_of_publics`].
pub fn sum_of_secrets(first: &[u8; 32], first_factor: &Hash, second: &[u8; 32], second_factor: &Hash) -> Option<[u8; 32]> {
    secret_of(scalar(first)? * scalar(first_factor)? + scalar(second)? * scalar(second_factor)?)
}

/// True for 33 bytes that name a point on the curve.
pub fn is_public_key(key: &PublicKey) -> bool {
    point(key).and_then(compressed).is_some_and(|again| again == *key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{public_key, sha256};

    #[test]
    fn a_moved_secret_still_matches_its_moved_key() {
        let secret = [3u8; 32];
        let tweak = sha256(b"tweak");
        let moved = add_to_secret(&secret, &tweak).unwrap();
        let key = public_key(&secret).unwrap();
        assert_eq!(public_key(&moved), add_to_public(&key, &tweak));
        let other = [5u8; 32];
        let other_tweak = sha256(b"other");
        let summed = sum_of_secrets(&secret, &tweak, &other, &other_tweak).unwrap();
        let other_key = public_key(&other).unwrap();
        assert_eq!(public_key(&summed), sum_of_publics(&key, &tweak, &other_key, &other_tweak));
        assert!(is_public_key(&key));
        assert!(!is_public_key(&[9u8; 33]));
    }
}
