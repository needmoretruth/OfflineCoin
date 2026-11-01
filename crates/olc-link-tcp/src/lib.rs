//! TCP for computers, on a home network, a VPN or the internet. Boards speak the same framing.
use olc_node::frame::{encode, Decoder};
use olc_node::network;
use olc_node::{Link, Target};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{IpAddr, Shutdown, SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

const READ_SIZE: usize = 16 * 1024;
const RECONNECT_DELAY: Duration = Duration::from_secs(2);
const ACCEPT_POLL: Duration = Duration::from_millis(100);

type Peers = Arc<Mutex<BTreeMap<u32, TcpStream>>>;
type Inbox = Sender<(u32, Vec<u8>)>;

pub struct TcpLink {
    peers: Peers,
    incoming: Receiver<(u32, Vec<u8>)>,
    sender: Inbox,
    next_peer: Arc<AtomicU32>,
    /// Goes up to end every `connect` started before.
    round: Arc<AtomicU64>,
    closed: Arc<AtomicBool>,
    no_control: Arc<AtomicBool>,
}

/// A handle that keeps working after the link has moved into a runner.
#[derive(Clone)]
pub struct TcpControl {
    peers: Peers,
    sender: Inbox,
    next_peer: Arc<AtomicU32>,
    round: Arc<AtomicU64>,
    closed: Arc<AtomicBool>,
    no_control: Arc<AtomicBool>,
}

impl TcpControl {
    /// The address of a peer, which a person may want to reach again.
    pub fn ip_of(&self, peer: u32) -> Option<IpAddr> {
        let peers = self.peers.lock().ok()?;
        peers.get(&peer)?.peer_addr().ok().map(|found| found.ip())
    }

    /// Keeps a connection to `address` open, reconnecting when it drops.
    pub fn connect(&self, address: String) {
        let (peers, sender, next_peer) = (
            self.peers.clone(),
            self.sender.clone(),
            self.next_peer.clone(),
        );
        let round = self.round.clone();
        let no_control = self.no_control.clone();
        let started_in = round.load(Ordering::Relaxed);
        thread::spawn(move || {
            while round.load(Ordering::Relaxed) == started_in {
                let target = address
                    .to_socket_addrs()
                    .ok()
                    .and_then(|mut found| found.next());
                if let Some(stream) = target.and_then(|target| TcpStream::connect(target).ok()) {
                    let refused = no_control.clone();
                    attach(stream, &peers, &sender, &next_peer, refused).join().ok();
                }
                thread::sleep(RECONNECT_DELAY);
            }
        });
    }

    /// Peers connected right now.
    pub fn count(&self) -> usize {
        self.peers.lock().map_or(0, |peers| peers.len())
    }

    /// Drops every peer and stops reconnecting, while still listening.
    pub fn drop_peers(&self) {
        self.round.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut peers) = self.peers.lock() {
            for stream in peers.values() {
                let _ = stream.shutdown(Shutdown::Both);
            }
            peers.clear();
        }
    }

    /// Stops listening as well.
    pub fn close(&self) {
        self.closed.store(true, Ordering::Relaxed);
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
        Self {
            peers: Peers::default(),
            incoming,
            sender,
            next_peer: Arc::default(),
            round: Arc::default(),
            closed: Arc::default(),
            no_control: Arc::default(),
        }
    }

    /// Control packets speak for boards and nothing proves who sent one. A connection from
    /// outside never delivers them. This stops the ones this link dialed as well.
    pub fn refuse_control(&self) {
        self.no_control.store(true, Ordering::Relaxed);
    }

    pub fn control(&self) -> TcpControl {
        TcpControl {
            peers: self.peers.clone(),
            sender: self.sender.clone(),
            next_peer: self.next_peer.clone(),
            round: self.round.clone(),
            closed: self.closed.clone(),
            no_control: self.no_control.clone(),
        }
    }

    /// Accepts peers on `address`, for example `0.0.0.0:7000`. Returns the address it got.
    pub fn listen(&self, address: &str) -> std::io::Result<SocketAddr> {
        let listener = TcpListener::bind(address)?;
        let local = listener.local_addr()?;
        listener.set_nonblocking(true)?;
        let (peers, sender, next_peer) = self.shared();
        let closed = self.closed.clone();
        let refused = Arc::new(AtomicBool::new(true));
        thread::spawn(move || {
            while !closed.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) if stream.set_nonblocking(false).is_ok() => {
                        attach(stream, &peers, &sender, &next_peer, refused.clone());
                    }
                    _ => thread::sleep(ACCEPT_POLL),
                }
            }
        });
        Ok(local)
    }

    /// Keeps a connection to `address` open, reconnecting when it drops.
    pub fn connect(&self, address: String) {
        self.control().connect(address);
    }

    fn shared(&self) -> (Peers, Inbox, Arc<AtomicU32>) {
        (
            self.peers.clone(),
            self.sender.clone(),
            self.next_peer.clone(),
        )
    }
}

fn attach(
    stream: TcpStream,
    peers: &Peers,
    sender: &Inbox,
    next_peer: &AtomicU32,
    no_control: Arc<AtomicBool>,
) -> thread::JoinHandle<()> {
    let peer = next_peer.fetch_add(1, Ordering::Relaxed);
    let _ = stream.set_nodelay(true);
    let writer = stream.try_clone();
    if let (Ok(writer), Ok(mut peers)) = (writer, peers.lock()) {
        peers.insert(peer, writer);
    }
    let peers = peers.clone();
    let sender = sender.clone();
    thread::spawn(move || {
        read_messages(stream, peer, &sender, &no_control);
        if let Ok(mut peers) = peers.lock() {
            peers.remove(&peer);
        }
    })
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
    fn send(&mut self, target: Target, message: &[u8]) {
        let Ok(mut peers) = self.peers.lock() else {
            return;
        };
        let frame = encode(message);
        peers.retain(|peer, stream| {
            let wanted = match target {
                Target::Peer(one) => *peer == one,
                Target::All => true,
                Target::AllExcept(other) => *peer != other,
            };
            !wanted || stream.write_all(&frame).is_ok()
        });
    }

    fn receive(&mut self) -> Option<(u32, Vec<u8>)> {
        self.incoming.try_recv().ok()
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
        link.connect(address.clone());
        let (mut board, _) = listener.accept().unwrap();
        say_both(&mut board);
        assert_eq!(heard(&mut link).len(), 2);

        link.refuse_control();
        say_both(&mut board);
        assert_eq!(heard(&mut link), vec![stamp(OPEN, b"hello")]);
    }
}
