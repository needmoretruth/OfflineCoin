//! TCP for computers, on a home network, a VPN or the internet. Boards speak the same framing.
use olc_node::frame::{encode, Decoder};
use olc_node::{Link, Target};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

const READ_SIZE: usize = 16 * 1024;
const RECONNECT_DELAY: Duration = Duration::from_secs(2);

type Peers = Arc<Mutex<BTreeMap<u32, TcpStream>>>;
type Inbox = Sender<(u32, Vec<u8>)>;

pub struct TcpLink {
    peers: Peers,
    incoming: Receiver<(u32, Vec<u8>)>,
    sender: Inbox,
    next_peer: Arc<AtomicU32>,
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
        }
    }

    /// Accepts peers on `address`, for example `0.0.0.0:7000`.
    pub fn listen(&self, address: &str) -> std::io::Result<()> {
        let listener = TcpListener::bind(address)?;
        let (peers, sender, next_peer) = self.shared();
        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                attach(stream, &peers, &sender, &next_peer);
            }
        });
        Ok(())
    }

    /// Keeps a connection to `address` open, reconnecting when it drops.
    pub fn connect(&self, address: String) {
        let (peers, sender, next_peer) = self.shared();
        thread::spawn(move || loop {
            let target = address
                .to_socket_addrs()
                .ok()
                .and_then(|mut found| found.next());
            if let Some(stream) = target.and_then(|target| TcpStream::connect(target).ok()) {
                attach(stream, &peers, &sender, &next_peer).join().ok();
            }
            thread::sleep(RECONNECT_DELAY);
        });
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
        read_messages(stream, peer, &sender);
        if let Ok(mut peers) = peers.lock() {
            peers.remove(&peer);
        }
    })
}

fn read_messages(mut stream: TcpStream, peer: u32, sender: &Inbox) {
    let mut decoder = Decoder::default();
    let mut bytes = vec![0u8; READ_SIZE];
    loop {
        let size = match stream.read(&mut bytes) {
            Ok(0) | Err(_) => return,
            Ok(size) => size,
        };
        decoder.push(&bytes[..size]);
        while let Some(message) = decoder.next_message() {
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
