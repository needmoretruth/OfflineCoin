//! Several networks can share one radio channel or LAN. Every message starts with a mark, as
//! Bitcoin's do with their magic bytes, so a node drops another network's traffic unread.
use alloc::vec::Vec;
use olc_core::Hash;

pub type Mark = [u8; 4];

/// Control traffic is signed and addressed to boards, so it crosses networks with its own mark:
/// a board on any coin stays reachable by the holder of its key.
pub const CONTROL: Mark = *b"CTRL";

pub fn mark_of(genesis: &Hash) -> Mark {
    [genesis[0], genesis[1], genesis[2], genesis[3]]
}

pub fn stamp(mark: Mark, message: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(mark.len() + message.len());
    bytes.extend_from_slice(&mark);
    bytes.extend_from_slice(message);
    bytes
}

/// Splits the mark from the message after it.
pub fn read(bytes: &[u8]) -> Option<(Mark, &[u8])> {
    let (mark, message) = bytes.split_first_chunk::<4>()?;
    Some((*mark, message))
}
