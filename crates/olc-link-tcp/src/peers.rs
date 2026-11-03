//! What a person sees of each connection, as Bitcoin Core's Peers tab shows it: who dialled whom,
//! since when, the bytes each way, the time a ping takes and the build at the far end. Also the
//! two things a person does to a peer: drop it, and keep its address out for a while.
use super::{Shared, TcpControl};
use olc_node::frame::encode;
use olc_node::network::{read, stamp, Mark};
use std::net::{IpAddr, Shutdown, SocketAddr};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// A ping and its answer. Nodes drop any mark they do not run, so an older build ignores both.
pub(super) const PING: Mark = *b"PING";
pub(super) const PONG: Mark = *b"PONG";
/// Each connection is pinged this often. Small next to the traffic every node sends each second.
pub(super) const PING_EVERY: Duration = Duration::from_secs(5);
/// The far end's build fits in this, so a stranger cannot fill memory with one.
const MOST_BUILD: usize = 32;
/// What this end says it runs.
const BUILD: &str = concat!("olc ", env!("CARGO_PKG_VERSION"));

/// One connection, as the peers table lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeerInfo {
    pub number: u32,
    /// The far end's address as this end sees it.
    pub address: Option<SocketAddr>,
    /// True when the far end dialled this one.
    pub inbound: bool,
    /// When the connection opened, in milliseconds since 1970.
    pub since_ms: u64,
    pub sent: u64,
    pub received: u64,
    /// The last ping's round trip, once one came back.
    pub ping_us: Option<u64>,
    /// What the far end says it runs, once it said.
    pub build: Option<String>,
}

/// Counts kept by the threads of one connection.
#[derive(Debug)]
pub(super) struct Stats {
    pub since_ms: u64,
    pub started: Instant,
    pub sent: AtomicU64,
    pub received: AtomicU64,
    /// Zero until a ping came back.
    pub ping_us: AtomicU64,
    pub build: Mutex<Option<String>>,
}

impl Stats {
    pub(super) fn new() -> Arc<Self> {
        let since_ms = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_millis() as u64);
        Arc::new(Self {
            since_ms,
            started: Instant::now(),
            sent: AtomicU64::default(),
            received: AtomicU64::default(),
            ping_us: AtomicU64::default(),
            build: Mutex::default(),
        })
    }

    /// A ping to send now: the time since the connection opened, then this end's build.
    pub(super) fn ping(&self) -> Vec<u8> {
        let mut body = (self.started.elapsed().as_micros() as u64).to_le_bytes().to_vec();
        body.extend_from_slice(BUILD.as_bytes());
        encode(&stamp(PING, &body))
    }

    /// Takes a ping or an answer to one, and gives the frame to send back, if any. Gives `None` as
    /// the first value for any other message, which goes on to the node.
    pub(super) fn heard(&self, message: &[u8]) -> Option<Option<Vec<u8>>> {
        let (mark, body) = read(message)?;
        if mark != PING && mark != PONG {
            return None;
        }
        let (time, build) = body.split_first_chunk::<8>()?;
        if !build.is_empty() {
            let text = String::from_utf8_lossy(&build[..build.len().min(MOST_BUILD)]).into_owned();
            if let Ok(mut slot) = self.build.lock() {
                *slot = Some(text);
            }
        }
        if mark == PING {
            let mut answer = time.to_vec();
            answer.extend_from_slice(BUILD.as_bytes());
            return Some(Some(encode(&stamp(PONG, &answer))));
        }
        let now = self.started.elapsed().as_micros() as u64;
        let trip = now.saturating_sub(u64::from_le_bytes(*time)).max(1);
        self.ping_us.store(trip, Ordering::Relaxed);
        Some(None)
    }
}

/// Addresses kept out, each until a moment.
pub(super) type Banned = Arc<Mutex<Vec<(IpAddr, Instant)>>>;

pub(super) fn is_banned(shared: &Shared, ip: IpAddr) -> bool {
    let Ok(mut banned) = shared.banned.lock() else {
        return false;
    };
    let now = Instant::now();
    banned.retain(|(_, until)| *until > now);
    banned.iter().any(|(kept_out, _)| *kept_out == ip)
}

impl TcpControl {
    /// Every connection now, oldest first.
    pub fn peers(&self) -> Vec<PeerInfo> {
        let Ok(peers) = self.0.peers.lock() else {
            return Vec::new();
        };
        peers
            .iter()
            .map(|(number, peer)| PeerInfo {
                number: *number,
                address: Some(peer.far),
                inbound: peer.accepted,
                since_ms: peer.stats.since_ms,
                sent: peer.stats.sent.load(Ordering::Relaxed),
                received: peer.stats.received.load(Ordering::Relaxed),
                ping_us: Some(peer.stats.ping_us.load(Ordering::Relaxed)).filter(|trip| *trip > 0),
                build: peer.stats.build.lock().ok().and_then(|build| build.clone()),
            })
            .collect()
    }

    /// Ends one connection. A node it dialled itself may dial it again later, as Bitcoin's does.
    pub fn disconnect(&self, number: u32) -> bool {
        let gone = self.0.peers.lock().ok().and_then(|mut peers| peers.remove(&number));
        if let Some(peer) = &gone {
            let _ = peer.stream.shutdown(Shutdown::Both);
        }
        gone.is_some()
    }

    /// Keeps `ip` out for `time`: its connections end now, and it can neither connect nor be
    /// dialled until then.
    pub fn ban(&self, ip: IpAddr, time: Duration) {
        if let Ok(mut banned) = self.0.banned.lock() {
            banned.retain(|(kept_out, _)| *kept_out != ip);
            banned.push((ip, Instant::now() + time));
        }
        if let Ok(mut peers) = self.0.peers.lock() {
            peers.retain(|_, peer| {
                let theirs = peer.far.ip() == ip;
                if theirs {
                    let _ = peer.stream.shutdown(Shutdown::Both);
                }
                !theirs
            });
        }
    }

    pub fn unban(&self, ip: IpAddr) {
        if let Ok(mut banned) = self.0.banned.lock() {
            banned.retain(|(kept_out, _)| *kept_out != ip);
        }
    }

    /// Addresses kept out now, with the seconds left for each.
    pub fn banned(&self) -> Vec<(IpAddr, u64)> {
        let Ok(mut banned) = self.0.banned.lock() else {
            return Vec::new();
        };
        let now = Instant::now();
        banned.retain(|(_, until)| *until > now);
        banned.iter().map(|(ip, until)| (*ip, until.duration_since(now).as_secs())).collect()
    }

    /// Where this link takes connections, if it does.
    pub fn listening_on(&self) -> Option<SocketAddr> {
        *self.0.bound.lock().ok()?
    }
}

#[cfg(test)]
mod tests {
    use super::super::TcpLink;
    use super::*;
    use olc_node::network::OPEN;
    use olc_node::{Link, Target};
    use std::net::TcpStream;
    use std::thread;

    fn wait_for(what: &str, check: impl Fn() -> bool) {
        let until = Instant::now() + Duration::from_secs(5);
        while !check() {
            assert!(Instant::now() < until, "{what}");
            thread::sleep(Duration::from_millis(5));
        }
    }

    /// Two links of this build: each lists the other with its direction, bytes, ping and build.
    #[test]
    fn each_side_lists_the_other_with_direction_bytes_ping_and_build() {
        let (mut here, mut there) = (TcpLink::new(), TcpLink::new());
        let address = there.listen("127.0.0.1:0").unwrap();
        here.dial(&address.to_string());
        let (near, far) = (here.control(), there.control());
        wait_for("both list a ping", || {
            let (ours, theirs) = (near.peers(), far.peers());
            ours.len() == 1 && theirs.len() == 1 && ours[0].ping_us.is_some() && theirs[0].ping_us.is_some()
        });
        here.send(Target::All, &stamp(OPEN, b"hello"));
        let until = Instant::now() + Duration::from_secs(2);
        while there.receive().is_none() {
            assert!(Instant::now() < until, "the message did not arrive");
            thread::sleep(Duration::from_millis(5));
        }
        let (ours, theirs) = (near.peers().remove(0), far.peers().remove(0));
        assert!(!ours.inbound && theirs.inbound);
        assert_eq!(ours.address, Some(address));
        assert!(ours.sent > 0 && theirs.received > 0);
        assert_eq!(theirs.build.as_deref(), Some(BUILD));
        assert!(ours.since_ms > 0);
    }

    #[test]
    fn a_disconnected_peer_goes_and_a_banned_one_cannot_come_back() {
        let link = TcpLink::new();
        let address = link.listen("127.0.0.1:0").unwrap();
        let control = link.control();
        let _first = TcpStream::connect(address).unwrap();
        wait_for("it connects", || control.count() == 1);
        let number = control.peers()[0].number;
        assert!(control.disconnect(number));
        wait_for("it goes", || control.count() == 0);

        let _second = TcpStream::connect(address).unwrap();
        wait_for("it connects again", || control.count() == 1);
        let ip = control.peers()[0].address.unwrap().ip();
        control.ban(ip, Duration::from_secs(3_600));
        assert_eq!(control.count(), 0);
        let _third = TcpStream::connect(address).unwrap();
        thread::sleep(Duration::from_millis(300));
        assert_eq!(control.count(), 0, "a banned address came back");
        assert_eq!(control.banned().len(), 1);
        assert!(control.banned()[0].1 > 3_500);
        control.unban(ip);
        let _fourth = TcpStream::connect(address).unwrap();
        wait_for("it connects once unbanned", || control.count() == 1);
    }
}
