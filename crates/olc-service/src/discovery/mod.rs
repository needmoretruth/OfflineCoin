//! Finding every OLC this computer can reach, with nothing inbound: it asks outward and it asks
//! nodes who else they know. Nothing here joins anything. A person picks.
mod group;
mod probe;
mod sweep;
mod udp;

pub use group::{group, ChainGroup, CoinGroup, Entry, Identity, Relation, Work};
pub use probe::ask;
pub use sweep::{Scanner, ScanPlan};
pub use udp::Shout;

use std::net::IpAddr;

/// How a thing is reached.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reach {
    ThisApp,
    ThisComputer,
    Usb,
    /// By radio, through the named board when known.
    Radio(Option<String>),
    Lan,
    Internet,
}

pub fn reach_of(ip: IpAddr) -> Reach {
    match ip {
        ip if ip.is_loopback() => Reach::ThisComputer,
        IpAddr::V4(ip) if ip.is_private() || ip.is_link_local() => Reach::Lan,
        IpAddr::V6(ip) if ip.is_unique_local() || ip.is_unicast_link_local() => Reach::Lan,
        _ => Reach::Internet,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_address_says_how_far_away_it_is() {
        let read = |text: &str| reach_of(text.parse().unwrap());
        assert_eq!(read("127.0.0.1"), Reach::ThisComputer);
        assert_eq!(read("192.168.0.4"), Reach::Lan);
        assert_eq!(read("10.1.2.3"), Reach::Lan);
        assert_eq!(read("8.8.8.8"), Reach::Internet);
    }
}
