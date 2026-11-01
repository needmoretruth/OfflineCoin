//! What boards and computers must agree on to find each other over Bluetooth. A node advertises
//! its id, and a connection is one L2CAP channel carrying the same framing as TCP.

/// The L2CAP channel every node listens on, in the range anyone may use.
pub const PSM: u16 = 0x0081;
/// The Bluetooth SIG keeps this company id for tests and unregistered makers.
pub const COMPANY: u16 = 0xFFFF;
const MARK: &[u8; 3] = b"OLC";
/// Bytes after the company id: the mark, then the node id.
pub const PAYLOAD: usize = 11;

/// What goes in the advertisement's manufacturer data.
pub fn payload(node: u64) -> [u8; PAYLOAD] {
    let mut bytes = [0u8; PAYLOAD];
    bytes[..3].copy_from_slice(MARK);
    bytes[3..].copy_from_slice(&node.to_le_bytes());
    bytes
}

/// The node id in manufacturer data, if this is one of ours.
pub fn node_of(payload: &[u8]) -> Option<u64> {
    let rest = payload.strip_prefix(MARK)?;
    Some(u64::from_le_bytes(rest.try_into().ok()?))
}

/// Looks through a whole advertisement for the node id.
pub fn node_in_advert(mut data: &[u8]) -> Option<u64> {
    const MANUFACTURER: u8 = 0xFF;
    while let Some((size, rest)) = data.split_first() {
        let size = usize::from(*size);
        if size == 0 || rest.len() < size {
            return None;
        }
        let (entry, after) = rest.split_at(size);
        if let [MANUFACTURER, low, high, payload @ ..] = entry {
            if u16::from_le_bytes([*low, *high]) == COMPANY {
                return node_of(payload);
            }
        }
        data = after;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_advert_gives_back_its_node() {
        let mut advert = alloc::vec![2, 0x01, 0x06, 14, 0xFF, 0xFF, 0xFF];
        advert.extend_from_slice(&payload(0x1122_3344_5566_7788));
        assert_eq!(node_in_advert(&advert), Some(0x1122_3344_5566_7788));
    }

    #[test]
    fn anything_else_on_the_air_is_passed_over() {
        assert_eq!(node_in_advert(&[2, 0x01, 0x06, 3, 0xFF, 0x4C, 0x00]), None);
        assert_eq!(node_in_advert(&[9, 0xFF]), None);
        assert_eq!(node_of(b"OLD12345678"), None);
    }
}
