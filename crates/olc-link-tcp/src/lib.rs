//! TCP for computers, on a home network, a VPN or the internet. Boards speak the same framing.
use olc_node::frame::{encode, Decoder};
use olc_node::network;
use olc_node::{Link, Medium, News, Target};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Write};
use std::net::{IpAddr, Shutdown, SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

const READ_SIZE: usize = 16 * 1024;
const ACCEPT_POLL: Duration = Duration::from_millis(100);
const DIAL_TIMEOUT: Duration = Duration::from_secs(4);
/// A peer that went out of range takes nothing, and the node must not wait for it.
const WRITE_TIMEOUT: Duration = Duration::from_secs(2);

type Peers = Arc<Mutex<BTreeMap<u32, TcpStream>>>;
type Inbox = Sender<(u32, Vec<u8>)>;

/// What threads share with the link.
#[derive(Clone)]
struct Shared {
    peers: Peers,
    sender: Inbox,
    news: Sender<News>,
    next_peer: Arc<AtomicU32>,
    /// Goes up to end every dial started before.
    round: Arc<AtomicU64>,
    closed: Arc<AtomicBool>,
    no_control: Arc<AtomicBool>,
    /// Dials given up while they were still connecting.
    cancelled: Arc<Mutex<BTreeSet<String>>>,
}

pub struct TcpLink {
    shared: Shared,
    incoming: Receiver<(u32, Vec<u8>)>,
    news: Receiver<News>,
}

/// A handle that keeps working after the link has moved into a runner.
#[derive(Clone)]
pub struct TcpControl(Shared);

impl TcpControl {
    /// The address of a peer, which a person may want to reach again.
    pub fn ip_of(&self, peer: u32) -> Option<IpAddr> {
        let peers = self.0.peers.lock().ok()?;
        peers.get(&peer)?.peer_addr().ok().map(|found| found.ip())
    }

    /// Peers connected right now.
    pub fn count(&self) -> usize {
        self.0.peers.lock().map_or(0, |peers| peers.len())
    }

    /// Drops every peer and ends every dial, while still listening.
    pub fn drop_peers(&self) {
        self.0.round.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut peers) = self.0.peers.lock() {
            for stream in peers.values() {
                let _ = stream.shutdown(Shutdown::Both);
            }
            peers.clear();
        }
    }

    /// Stops listening as well.
    pub fn close(&self) {
        self.0.closed.store(true, Ordering::Relaxed);
        self.drop_peers();
    }
}

impl Default for TcpLink {
    fn default() -> Self {
        Self::new()
    }
}

impl TcpLink {
    pub fn new() -> Self {
        let (sender, incoming) = channel();
        let (news_sender, news) = channel();
        let shared = Shared {
            peers: Peers::default(),
            sender,
            news: news_sender,
            next_peer: Arc::default(),
            round: Arc::default(),
            closed: Arc::default(),
            no_control: Arc::default(),
            cancelled: Arc::default(),
        };
        Self {
            shared,
            incoming,
            news,
        }
    }

    /// Control packets speak for boards and nothing proves who sent one. A connection from
    /// outside never delivers them. This stops the ones this link dialed as well.
    pub fn refuse_control(&self) {
        self.shared.no_control.store(true, Ordering::Relaxed);
    }

    pub fn control(&self) -> TcpControl {
        TcpControl(self.shared.clone())
    }

    /// Accepts peers on `address`, for example `0.0.0.0:7000`. Returns the address it got.
    pub fn listen(&self, address: &str) -> std::io::Result<SocketAddr> {
        let listener = TcpListener::bind(address)?;
        let local = listener.local_addr()?;
        listener.set_nonblocking(true)?;
        let shared = self.shared.clone();
        let refused = Arc::new(AtomicBool::new(true));
        thread::spawn(move || {
            while !shared.closed.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) if stream.set_nonblocking(false).is_ok() => {
                        attach(stream, &shared, refused.clone(), None);
                    }
                    _ => thread::sleep(ACCEPT_POLL),
                }
            }
        });
        Ok(local)
    }
}

/// One try. The answer comes back as news.
fn dial(shared: Shared, address: String) {
    let started_in = shared.round.load(Ordering::Relaxed);
    let target = address.to_socket_addrs().ok().and_then(|mut found| found.next());
    let stream = target.and_then(|target| TcpStream::connect_timeout(&target, DIAL_TIMEOUT).ok());
    let cancelled = shared.cancelled.lock().is_ok_and(|mut set| set.remove(&address));
    match stream {
        Some(stream) if !cancelled && shared.round.load(Ordering::Relaxed) == started_in => {
            let refused = shared.no_control.clone();
            attach(stream, &shared, refused, Some(address));
        }
        _ => {
            let _ = shared.news.send(News::Failed { address });
        }
    }
}

fn attach(stream: TcpStream, shared: &Shared, no_control: Arc<AtomicBool>, address: Option<String>) {
    let peer = shared.next_peer.fetch_add(1, Ordering::Relaxed);
    let _ = stream.set_nodelay(true);
    let _ = stream.set_write_timeout(Some(WRITE_TIMEOUT));
    let writer = stream.try_clone();
    if let (Ok(writer), Ok(mut peers)) = (writer, shared.peers.lock()) {
        peers.insert(peer, writer);
    }
    let _ = shared.news.send(News::Opened { number: peer, address });
    let shared = shared.clone();
    thread::spawn(move || {
        read_messages(stream, peer, &shared.sender, &no_control);
        if let Ok(mut peers) = shared.peers.lock() {
            peers.remove(&peer);
        }
        let _ = shared.news.send(News::Closed { number: peer });
    });
}

fn is_control(message: &[u8]) -> bool {
    network::read(message).is_some_and(|(mark, _)| mark == network::CONTROL)
}

fn read_messages(mut stream: TcpStream, peer: u32, sender: &Inbox, no_control: &AtomicBool) {
    let mut decoder = Decoder::default();
    let mut bytes = vec![0u8; READ_SIZE];
    loop {
        let size = match stream.read(&mut bytes) {
            Ok(0) | Err(_) => return,
            Ok(size) => size,
        };
        decoder.push(&bytes[..size]);
        while let Some(message) = decoder.next_message() {
            if no_control.load(Ordering::Relaxed) && is_control(&message) {
                continue;
            }
            if sender.send((peer, message)).is_err() {
                return;
            }
        }
        if decoder.is_broken() {
            return;
        }
    }
}

impl Link for TcpLink {
    fn medium(&self) -> Medium {
        Medium::Wifi
    }

    fn send(&mut self, target: Target, message: &[u8]) {
        let Ok(mut peers) = self.shared.peers.lock() else {
            return;
        };
        let frame = encode(message);
        peers.retain(|peer, stream| {
            let wanted = match target {
                Target::Peer(one) => *peer == one,
                Target::All => true,
                Target::AllExcept(other) => *peer != other,
            };
            if !wanted || stream.write_all(&frame).is_ok() {
                return true;
            }
            // Half a frame may have left, so this connection can never be read again.
            let _ = stream.shutdown(Shutdown::Both);
            false
        });
    }

    fn receive(&mut self) -> Option<(u32, Vec<u8>)> {
        self.incoming.try_recv().ok()
    }

    fn dial(&mut self, address: &str) {
        if let Ok(mut set) = self.shared.cancelled.lock() {
            set.remove(address);
        }
        let (shared, address) = (self.shared.clone(), address.to_string());
        thread::spawn(move || dial(shared, address));
    }

    fn cancel(&mut self, address: &str) {
        if let Ok(mut set) = self.shared.cancelled.lock() {
            set.insert(address.to_string());
        }
    }

    fn close(&mut self, number: u32) {
        if let Some(stream) = self.shared.peers.lock().ok().and_then(|mut peers| peers.remove(&number)) {
            let _ = stream.shutdown(Shutdown::Both);
        }
    }

    fn news(&mut self) -> Option<News> {
        self.news.try_recv().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use olc_node::network::{stamp, CONTROL, OPEN};
    use std::time::Instant;

    fn heard(link: &mut TcpLink) -> Vec<Vec<u8>> {
        let until = Instant::now() + Duration::from_millis(400);
        let mut messages = Vec::new();
        while Instant::now() < until {
            match link.receive() {
                Some((_, message)) => messages.push(message),
                None => thread::sleep(Duration::from_millis(10)),
            }
        }
        messages
    }

    fn say_both(stream: &mut TcpStream) {
        stream.write_all(&encode(&stamp(CONTROL, b"beacon"))).unwrap();
        stream.write_all(&encode(&stamp(OPEN, b"hello"))).unwrap();
    }

    #[test]
    fn a_stranger_cannot_deliver_control_packets() {
        let mut link = TcpLink::new();
        let address = link.listen("127.0.0.1:0").unwrap();
        let mut stranger = TcpStream::connect(address).unwrap();
        say_both(&mut stranger);
        assert_eq!(heard(&mut link), vec![stamp(OPEN, b"hello")]);
    }

    #[test]
    fn a_dialed_connection_can_until_it_is_refused_too() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let mut link = TcpLink::new();
        Link::dial(&mut link, &address);
        let (mut board, _) = listener.accept().unwrap();
        say_both(&mut board);
        assert_eq!(heard(&mut link).len(), 2);

        link.refuse_control();
        say_both(&mut board);
        assert_eq!(heard(&mut link), vec![stamp(OPEN, b"hello")]);
    }

    fn next_news(link: &mut TcpLink) -> News {
        let until = Instant::now() + Duration::from_secs(6);
        while Instant::now() < until {
            match link.news() {
                Some(news) => return news,
                None => thread::sleep(Duration::from_millis(5)),
            }
        }
        panic!("no news");
    }

    #[test]
    fn a_dial_says_how_it_went_and_a_closed_connection_says_so() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let mut link = TcpLink::new();
        Link::dial(&mut link, &address);
        let (far_end, _) = listener.accept().unwrap();
        let opened = next_news(&mut link);
        let News::Opened { number, address: Some(dialled) } = opened else {
            panic!("{opened:?}");
        };
        assert_eq!(dialled, address);
        drop(far_end);
        assert_eq!(next_news(&mut link), News::Closed { number });
    }

    #[test]
    fn a_place_nobody_listens_at_fails_without_retrying() {
        let unused = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().to_string();
        let mut link = TcpLink::new();
        Link::dial(&mut link, &unused);
        assert_eq!(next_news(&mut link), News::Failed { address: unused });
        thread::sleep(Duration::from_millis(100));
        assert_eq!(link.news(), None);
    }

    #[test]
    fn a_cancelled_dial_leaves_no_connection() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let mut link = TcpLink::new();
        link.cancel(&address);
        let shared = link.shared.clone();
        dial(shared, address.clone());
        assert_eq!(next_news(&mut link), News::Failed { address });
        assert_eq!(link.control().count(), 0);
    }
}
