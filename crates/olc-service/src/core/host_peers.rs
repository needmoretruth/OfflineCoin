//! Who this node is connected to, as Bitcoin Core's Peers tab lists them: how, which way, since
//! when, the bytes each way, the ping and the build. Also where others reach this node, and the
//! orders a person gives about a peer: drop it, keep its address out for a while, let it back.
use olc_link_tcp::TcpControl;
use olc_node::{Medium, Runner};
use std::net::IpAddr;
use std::time::Duration;

/// What carries a connection, said truly: the network link is TCP whatever is under it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Carried {
    #[default]
    Tcp,
    Usb,
    Radio,
}

impl Carried {
    pub fn of(medium: Medium) -> Self {
        match medium {
            Medium::Wifi => Self::Tcp,
            Medium::Wire => Self::Usb,
            Medium::EspNow | Medium::Bluetooth => Self::Radio,
        }
    }

    /// The word the command line prints.
    pub fn word(self) -> &'static str {
        match self {
            Self::Tcp => "tcp",
            Self::Usb => "usb",
            Self::Radio => "radio",
        }
    }
}

/// One connection.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PeerRow {
    /// Its number on the network link. Rows on other links have none: nothing here drops them.
    pub number: Option<u32>,
    pub carried: Carried,
    /// `host:port` as this end sees it, for a TCP connection.
    pub address: String,
    pub inbound: bool,
    /// Milliseconds since 1970, when known.
    pub since_ms: Option<u64>,
    pub sent: u64,
    pub received: u64,
    pub ping_us: Option<u64>,
    /// What the far end runs, once it said.
    pub build: String,
    /// The name and key its channel node goes by, once it said hello.
    pub name: String,
    pub key: String,
}

/// The network as the peers list shows it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PeersView {
    pub rows: Vec<PeerRow>,
    /// Addresses kept out, with the seconds left.
    pub banned: Vec<(String, u64)>,
    /// `host:port` others dial to reach this node.
    pub reach_at: Option<String>,
    /// True when only programs on this computer can connect.
    pub only_here: bool,
}

/// The address another computer dials: this computer's own on its network when the node listens
/// on every address, else the one it listens on.
pub(super) fn reach_at(control: &TcpControl) -> (Option<String>, bool) {
    let Some(bound) = control.listening_on() else {
        return (None, true);
    };
    let port = bound.port();
    match bound.ip() {
        ip if ip.is_loopback() => (Some(format!("{ip}:{port}")), true),
        ip if ip.is_unspecified() => {
            let lan = crate::discovery::local_networks().into_iter().next();
            match lan {
                Some(lan) => (Some(format!("{lan}:{port}")), false),
                None => (Some(format!("127.0.0.1:{port}")), true),
            }
        }
        ip => (Some(format!("{ip}:{port}")), false),
    }
}

/// Every connection of the node now. `named` gives the channel name and key heard on a TCP
/// connection by its number.
pub(super) fn peers_of(control: &TcpControl, runner: &Runner, named: &dyn Fn(u32) -> Option<(String, String)>) -> PeersView {
    let mut view = PeersView::default();
    (view.reach_at, view.only_here) = reach_at(control);
    for peer in control.peers() {
        let (name, key) = named(peer.number).unwrap_or_default();
        view.rows.push(PeerRow {
            number: Some(peer.number),
            carried: Carried::Tcp,
            address: peer.address.map(|address| address.to_string()).unwrap_or_default(),
            inbound: peer.inbound,
            since_ms: Some(peer.since_ms),
            sent: peer.sent,
            received: peer.received,
            ping_us: peer.ping_us,
            build: peer.build.unwrap_or_default(),
            name,
            key,
        });
    }
    view.banned = control.banned().into_iter().map(|(ip, left)| (ip.to_string(), left)).collect();
    // Boards on a cable or a radio: the link they use now, with nothing to count on it here.
    for neighbour in runner.neighbours() {
        let medium = neighbour.active.and_then(|link| runner.medium_of(link));
        let Some(carried) = medium.map(Carried::of).filter(|carried| *carried != Carried::Tcp) else {
            continue;
        };
        view.rows.push(PeerRow { carried, ..PeerRow::default() });
    }
    view
}

/// Carries out an order about a peer. Gives the notice's key, its detail and whether it went.
pub(super) fn peer_order(control: &TcpControl, order: PeerOrder) -> (&'static str, String, bool) {
    match order {
        PeerOrder::Disconnect(number) => match control.disconnect(number) {
            true => ("ch.disconnected", String::new(), true),
            false => ("ch.no_such_peer", String::new(), false),
        },
        PeerOrder::Ban { address, hours } => {
            let Some(ip) = ip_of(&address) else {
                return ("ch.bad_ip", address, false);
            };
            control.ban(ip, Duration::from_secs(hours.clamp(1, 24 * 365) * 3_600));
            ("ch.banned", ip.to_string(), true)
        }
        PeerOrder::Unban(address) => {
            let Some(ip) = ip_of(&address) else {
                return ("ch.bad_ip", address, false);
            };
            control.unban(ip);
            ("ch.unbanned", ip.to_string(), true)
        }
    }
}

pub(super) enum PeerOrder {
    Disconnect(u32),
    Ban { address: String, hours: u64 },
    Unban(String),
}

/// The connections as `olc peers` prints them, one a line, like bitcoin-cli getpeerinfo cut short.
pub(super) fn peer_listing(view: &PeersView) -> String {
    let mut lines = Vec::new();
    for row in &view.rows {
        let number = row.number.map_or_else(|| "-".to_string(), |number| number.to_string());
        let way = if row.inbound { "in" } else { "out" };
        let kind = row.carried.word();
        let ping = row.ping_us.map_or_else(|| "-".to_string(), |us| format!("{:.1} ms", Duration::from_micros(us).as_secs_f64() * 1_000.0));
        let mut line = format!("{number} {kind} {way} {} ping {ping} sent {} received {}", row.address, row.sent, row.received);
        for (label, value) in [("build", &row.build), ("name", &row.name)] {
            if !value.is_empty() {
                line.push_str(&format!(" {label} {value}"));
            }
        }
        lines.push(line);
    }
    if lines.is_empty() {
        lines.push("No connections.".to_string());
    }
    lines.extend(view.banned.iter().map(|(ip, left)| format!("Banned {ip} for {left} s more")));
    lines.join("\n")
}

/// The IP in `1.2.3.4`, `1.2.3.4:7000` or `[::1]:7000`.
fn ip_of(text: &str) -> Option<IpAddr> {
    let text = text.trim();
    if let Ok(ip) = text.parse() {
        return Some(ip);
    }
    text.parse::<std::net::SocketAddr>().ok().map(|address| address.ip())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_address_reads_with_or_without_its_port() {
        assert_eq!(ip_of("127.0.0.1"), "127.0.0.1".parse().ok());
        assert_eq!(ip_of(" 10.0.0.2:7000 "), "10.0.0.2".parse().ok());
        assert_eq!(ip_of("[::1]:7000"), "::1".parse().ok());
        assert_eq!(ip_of("nowhere"), None);
    }

    fn wait_for(what: &str, done: impl Fn() -> bool) {
        let until = std::time::Instant::now() + Duration::from_secs(10);
        while !done() {
            assert!(std::time::Instant::now() < until, "{what} did not happen");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// The orders the peers list and `olc disconnect`, `ban` and `unban` give reach the node's own
    /// link, which the host holds, and the listing names the number that drops each connection.
    #[test]
    fn a_connection_is_listed_dropped_and_its_address_kept_out_then_let_back() {
        let link = olc_link_tcp::TcpLink::new();
        let address = link.listen("127.0.0.1:0").unwrap();
        let control = link.control();
        let _first = std::net::TcpStream::connect(address).unwrap();
        wait_for("the first connection", || control.count() == 1);
        let number = control.peers()[0].number;
        let view = PeersView { rows: vec![PeerRow { number: Some(number), address: "127.0.0.1:5000".into(), inbound: true, ..PeerRow::default() }], ..PeersView::default() };
        assert!(peer_listing(&view).starts_with(&format!("{number} tcp in 127.0.0.1:5000")), "{}", peer_listing(&view));
        assert_eq!(peer_order(&control, PeerOrder::Disconnect(number)).0, "ch.disconnected");
        wait_for("the drop", || control.count() == 0);
        assert_eq!(peer_order(&control, PeerOrder::Disconnect(number)).0, "ch.no_such_peer");
        assert_eq!(peer_order(&control, PeerOrder::Ban { address: "127.0.0.1".into(), hours: 24 }).0, "ch.banned");
        let left = control.banned().first().map_or(0, |(_, left)| *left);
        assert!(left > 23 * 3_600, "a day's ban has {left} s left");
        assert_eq!(peer_order(&control, PeerOrder::Ban { address: "nowhere".into(), hours: 1 }).0, "ch.bad_ip");
        assert_eq!(peer_order(&control, PeerOrder::Unban("127.0.0.1".into())).0, "ch.unbanned");
        assert!(control.banned().is_empty());
        control.close();
    }

    #[test]
    fn the_network_link_reads_as_tcp_and_cables_as_usb() {
        assert_eq!(Carried::of(Medium::Wifi).word(), "tcp");
        assert_eq!(Carried::of(Medium::Wire).word(), "usb");
        assert_eq!(Carried::of(Medium::EspNow).word(), "radio");
        assert_eq!(Carried::of(Medium::Bluetooth).word(), "radio");
    }
}
