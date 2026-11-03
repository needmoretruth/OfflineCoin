//! A request for a payment, after BOLT 11: how much, the hash whose preimage pays it, who is paid,
//! until when, signed by the payee. It travels as text or a QR code of that text.
use crate::bytes::{from_hex, hex, Bytes, Signature};
use crate::wire::NodeId;
use olc_crypto::{hash, sign, verify, Hash};
use serde::{Deserialize, Serialize};

pub const TEXT_START: &str = "olcpay1";
/// Longest note a person can put on a request.
pub const MOST_NOTE: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Invoice {
    pub payee: NodeId,
    pub amount: u64,
    pub hash: Hash,
    /// Seconds since 1970 when it was made, and how many it stays good for.
    pub made: u64,
    pub lasts: u32,
    /// Blocks the last payment must still have before its expiry.
    pub final_delta: u16,
    /// The first bytes of the coin's genesis id, so it is not paid on another coin.
    pub coin: [u8; 8],
    pub note: String,
    pub signature: Signature,
}

#[derive(Debug, PartialEq, Eq)]
pub enum InvoiceError {
    NotOne,
    BadSignature,
    OtherCoin,
    Expired,
}

impl Invoice {
    fn digest(&self) -> Hash {
        let mut unsigned = self.clone();
        unsigned.signature = Bytes([0; 64]);
        hash(&postcard::to_allocvec(&unsigned).unwrap_or_default())
    }

    pub fn signed(mut self, secret: &[u8; 32]) -> Self {
        self.note.truncate(MOST_NOTE);
        self.signature = Bytes(sign(secret, &self.digest()).unwrap_or([0; 64]));
        self
    }

    pub fn to_text(&self) -> String {
        format!("{TEXT_START}{}", hex(&postcard::to_allocvec(self).unwrap_or_default()))
    }

    /// Reads a request and checks it: signed by its payee, for this coin, still good at `now`.
    pub fn read(text: &str, coin: &[u8; 8], now: u64) -> Result<Self, InvoiceError> {
        let text = text.trim().to_lowercase();
        let body = text.strip_prefix(TEXT_START).ok_or(InvoiceError::NotOne)?;
        let bytes = from_hex(body).ok_or(InvoiceError::NotOne)?;
        let invoice: Self = postcard::from_bytes(&bytes).map_err(|_| InvoiceError::NotOne)?;
        if !verify(&invoice.payee.0, &invoice.digest(), &invoice.signature.0) {
            return Err(InvoiceError::BadSignature);
        }
        if &invoice.coin != coin {
            return Err(InvoiceError::OtherCoin);
        }
        if invoice.expired(now) {
            return Err(InvoiceError::Expired);
        }
        Ok(invoice)
    }

    pub fn expired(&self, now: u64) -> bool {
        now >= self.made.saturating_add(u64::from(self.lasts))
    }
}

/// The coin tag of a genesis id.
pub fn coin_tag(genesis: &Hash) -> [u8; 8] {
    let mut tag = [0u8; 8];
    tag.copy_from_slice(&genesis[..8]);
    tag
}

#[cfg(test)]
mod tests {
    use super::*;
    use olc_crypto::{public_key, sha256};

    #[test]
    fn a_request_reads_back_only_as_it_was_signed() {
        let secret = [4u8; 32];
        let invoice = Invoice {
            payee: Bytes(public_key(&secret).unwrap()),
            amount: 5_000,
            hash: sha256(b"preimage"),
            made: 1_000,
            lasts: 3_600,
            final_delta: 18,
            coin: [1; 8],
            note: "coffee".into(),
            signature: Bytes([0; 64]),
        }
        .signed(&secret);
        let text = invoice.to_text();
        assert!(text.starts_with(TEXT_START));
        assert_eq!(Invoice::read(&text, &[1; 8], 2_000), Ok(invoice.clone()));
        assert_eq!(Invoice::read(&text, &[2; 8], 2_000), Err(InvoiceError::OtherCoin));
        assert_eq!(Invoice::read(&text, &[1; 8], 4_600), Err(InvoiceError::Expired));
        let mut changed = invoice;
        changed.amount = 1;
        assert_eq!(Invoice::read(&changed.to_text(), &[1; 8], 2_000), Err(InvoiceError::BadSignature));
        assert_eq!(Invoice::read("olcpay1zz", &[1; 8], 0), Err(InvoiceError::NotOne));
    }
}
