//! Hears the ids boards broadcast on a router's network (`olc_node::lan`), so a computer on the
//! same router dials them with no address typed. Only addresses on the local network are dialled:
//! a datagram from anywhere else names no neighbour.
use super::Shared;
use olc_node::lan::heard;
use olc_node::News;
pub use olc_platform::shared_udp;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::atomic::Ordering;
use std::thread;
use std::time::Duration;

/// The port boards broadcast to.
pub const PORT: u16 = 7000;
const READ_WAIT: Duration = Duration::from_millis(500);

fn nearby(ip: Ipv4Addr) -> bool {
    ip.is_private() || ip.is_link_local() || ip.is_loopback()
}

pub(super) fn start(shared: Shared, port: u16, me: u64) -> std::io::Result<()> {
    let socket = shared_udp(port)?;
    socket.set_read_timeout(Some(READ_WAIT))?;
    thread::spawn(move || {
        let mut bytes = [0u8; 64];
        while !shared.closed.load(Ordering::Relaxed) {
            let Ok((size, SocketAddr::V4(from))) = socket.recv_from(&mut bytes) else {
                continue;
            };
            if !nearby(*from.ip()) {
                continue;
            }
            let sender = SocketAddrV4::new(*from.ip(), from.port());
            if let Some((node, address)) = heard(&bytes[..size], sender, me) {
                if shared.news.send(News::Seen { node, address }).is_err() {
                    return;
                }
            }
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::TcpLink;
    use olc_node::lan::announcement;
    use olc_node::{Link, News};
    use std::net::UdpSocket;
    use std::time::{Duration, Instant};

    const ME: u64 = 0x1111;
    const BOARD: u64 = 0x0000_78ef_aa6e_27ac;

    fn seen(link: &mut TcpLink) -> Option<News> {
        let until = Instant::now() + Duration::from_secs(3);
        while Instant::now() < until {
            match link.news() {
                Some(news @ News::Seen { .. }) => return Some(news),
                _ => std::thread::sleep(Duration::from_millis(20)),
            }
        }
        None
    }

    #[test]
    fn a_computer_hears_a_board_on_the_router_and_learns_where_to_dial() {
        let port = UdpSocket::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let mut link = TcpLink::new();
        link.hear_neighbours(port, ME).expect("hearing");
        let board = UdpSocket::bind("127.0.0.1:0").unwrap();
        let from = board.local_addr().unwrap();
        board.send_to(&announcement(ME), ("127.0.0.1", port)).unwrap();
        board.send_to(&announcement(BOARD), ("127.0.0.1", port)).unwrap();
        assert_eq!(seen(&mut link), Some(News::Seen { node: BOARD, address: format!("{from}") }));
    }

    #[test]
    fn the_apps_look_around_and_its_node_share_the_port() {
        let port = UdpSocket::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let _first = super::shared_udp(port).expect("one socket");
        let _second = super::shared_udp(port).expect("a second socket on the same port");
    }
}
