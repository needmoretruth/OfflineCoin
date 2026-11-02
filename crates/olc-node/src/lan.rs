//! What nodes on one router's network must agree on to find each other. A node that takes
//! connections broadcasts its id now and then, from the port it takes them on, and whoever hears
//! it can dial the sender's address. Routers hand out addresses that change, so nobody types one.
use crate::message::Message;
use crate::network::{read, stamp, OPEN};
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::net::SocketAddrV4;

/// How often a node says it is there. A sighting counts as fresh for five seconds when dialling.
pub const SAY_EVERY_MS: u64 = 2_000;

/// What a node broadcasts: the id it goes by on every link. Any coin, since the connection that
/// follows says which.
pub fn announcement(node: u64) -> Vec<u8> {
    stamp(OPEN, &Message::Here(node).encode())
}

/// The node behind an announcement and the address that reaches it, unless it is this node's own
/// or not an announcement at all.
pub fn heard(bytes: &[u8], from: SocketAddrV4, me: u64) -> Option<(u64, String)> {
    let (mark, body) = read(bytes)?;
    if mark != OPEN || from.port() == 0 {
        return None;
    }
    match Message::decode(body)? {
        Message::Here(node) if node != me => Some((node, format!("{from}"))),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::mark_of;

    const ME: u64 = 0x0000_04e2_aa12_3456;
    const THEM: u64 = 0x0000_78ef_aa65_4321;

    fn from(text: &str) -> SocketAddrV4 {
        text.parse().unwrap()
    }

    #[test]
    fn a_neighbour_on_the_router_is_found_at_the_address_it_spoke_from() {
        let said = announcement(THEM);
        let found = heard(&said, from("192.168.77.103:7000"), ME);
        assert_eq!(found, Some((THEM, "192.168.77.103:7000".into())));
    }

    #[test]
    fn its_own_announcement_and_strangers_traffic_find_nobody() {
        assert_eq!(heard(&announcement(ME), from("192.168.77.102:7000"), ME), None);
        let other_coin = stamp(mark_of(&[7; 32]), &Message::Here(THEM).encode());
        assert_eq!(heard(&other_coin, from("192.168.77.103:7000"), ME), None);
        let not_here = stamp(OPEN, &Message::Who.encode());
        assert_eq!(heard(&not_here, from("192.168.77.103:7000"), ME), None);
        assert_eq!(heard(b"OPEN\xff\xff", from("192.168.77.103:7000"), ME), None);
        assert_eq!(heard(&announcement(THEM), from("192.168.77.103:0"), ME), None);
    }

    #[test]
    fn an_announcement_fits_one_small_datagram() {
        assert!(announcement(THEM).len() <= 32);
    }
}
