//! TCP for computers, on a home network, a VPN or the internet. Boards speak the same framing.
pub mod lan;
mod peers;

pub use peers::PeerInfo;
use peers::{is_banned, Banned, Stats, PING_EVERY};

use olc_node::frame::{encode, Decoder};
use olc_node::network;
use olc_node::{Link, Medium, News, Target};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{ErrorKind, Read, Write};
use std::net::{IpAddr, Shutdown, SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{channel, sync_channel, Receiver, Sender, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const READ_SIZE: usize = 16 * 1024;
const ACCEPT_POLL: Duration = Duration::from_millis(100);
const DIAL_TIMEOUT: Duration = Duration::from_secs(4);
/// A peer that went out of range takes nothing, and its writer gives up on it after this long.
const WRITE_TIMEOUT: Duration = Duration::from_secs(2);
/// Connections taken from outside at once. Anyone can open one, and each costs two threads and up
/// to a whole message of memory. What this node dials itself does not count.
const MOST_ACCEPTED: usize = 32;
/// Whole messages waiting for the node. Past this the readers stop, and the far ends wait on
/// their own sockets.
const INBOX: usize = 1_024;
/// Frames waiting to go to one peer. One this far behind is not keeping up, and goes.
const OUTBOX: usize = 256;
/// A connection that brings no whole message for this long goes. Every node speaks each second,
/// so this only ends one that went away without saying, or one that only takes up a place.
const IDLE: Duration = if cfg!(test) { Duration::from_secs(1) } else { Duration::from_secs(30) };

type Frame = Arc<[u8]>;
type Peers = Arc<Mutex<BTreeMap<u32, Peer>>>;
type Inbox = SyncSender<(u32, Vec<u8>)>;

struct Peer {
    stream: TcpStream,
    /// The far end, noted when the connection began: Windows does not give it back from a copy
    /// of a socket that was dialled with a time limit.
    far: SocketAddr,
    frames: SyncSender<Frame>,
    accepted: bool,
    stats: Arc<Stats>,
}

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
    /// The listener taking connections now, if any.
    listening: Arc<Mutex<Option<Listening>>>,
    /// Where it listens.
    bound: Arc<Mutex<Option<SocketAddr>>>,
    banned: Banned,
}

/// One listener's thread, and the flag that ends it.
struct Listening {
    stop: Arc<AtomicBool>,
    thread: thread::JoinHandle<()>,
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
        peers.get(&peer).map(|found| found.far.ip())
    }

    /// Peers connected right now.
    pub fn count(&self) -> usize {
        self.0.peers.lock().map_or(0, |peers| peers.len())
    }

    /// Drops every peer and ends every dial, while still listening.
    pub fn drop_peers(&self) {
        self.0.round.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut peers) = self.0.peers.lock() {
            for peer in peers.values() {
                let _ = peer.stream.shutdown(Shutdown::Both);
            }
            peers.clear();
        }
    }

    /// Listens on `address` instead of where it listened before, for example when a person no
    /// longer lets others on the network connect. Peers taken by the old listener go.
    pub fn listen_again(&self, address: &str) -> std::io::Result<SocketAddr> {
        let old = self.0.listening.lock().ok().and_then(|mut slot| slot.take());
        if let Some(old) = old {
            old.stop.store(true, Ordering::Relaxed);
            // The port is free again only once that thread has let go of its socket.
            let _ = old.thread.join();
        }
        if let Ok(peers) = self.0.peers.lock() {
            for peer in peers.values().filter(|peer| peer.accepted) {
                let _ = peer.stream.shutdown(Shutdown::Both);
            }
        }
        start_listening(&self.0, address)
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
        let (sender, incoming) = sync_channel(INBOX);
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
            listening: Arc::default(),
            bound: Arc::default(),
            banned: Arc::default(),
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

    /// Hears the ids boards broadcast to `port` on a router's network and reports them, so the
    /// node dials them. `me` is this node's id, which it hears from itself and ignores.
    pub fn hear_neighbours(&self, port: u16, me: u64) -> std::io::Result<()> {
        lan::start(self.shared.clone(), port, me)
    }

    /// Accepts peers on `address`, for example `0.0.0.0:7000`. Returns the address it got.
    pub fn listen(&self, address: &str) -> std::io::Result<SocketAddr> {
        start_listening(&self.shared, address)
    }
}

fn start_listening(shared: &Shared, address: &str) -> std::io::Result<SocketAddr> {
    let listener = TcpListener::bind(address)?;
    let local = listener.local_addr()?;
    if let Ok(mut bound) = shared.bound.lock() {
        *bound = Some(local);
    }
    listener.set_nonblocking(true)?;
    let stop = Arc::new(AtomicBool::new(false));
    let (taking, ends) = (shared.clone(), stop.clone());
    let refused = Arc::new(AtomicBool::new(true));
    let handle = thread::spawn(move || {
        while !taking.closed.load(Ordering::Relaxed) && !ends.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((_, from)) if is_banned(&taking, from.ip()) => {}
                Ok((stream, from)) if has_room(&taking) && stream.set_nonblocking(false).is_ok() => {
                    attach(stream, from, &taking, refused.clone(), None);
                }
                _ => thread::sleep(ACCEPT_POLL),
            }
        }
    });
    if let Ok(mut slot) = shared.listening.lock() {
        *slot = Some(Listening { stop, thread: handle });
    }
    Ok(local)
}

fn has_room(shared: &Shared) -> bool {
    let accepted = |peers: &BTreeMap<u32, Peer>| peers.values().filter(|peer| peer.accepted).count();
    shared.peers.lock().is_ok_and(|peers| accepted(&peers) < MOST_ACCEPTED)
}

/// One try. The answer comes back as news.
fn dial(shared: Shared, address: String) {
    let started_in = shared.round.load(Ordering::Relaxed);
    let target = address.to_socket_addrs().ok().and_then(|mut found| found.next());
    let target = target.filter(|target| !is_banned(&shared, target.ip()));
    let stream = target.and_then(|target| TcpStream::connect_timeout(&target, DIAL_TIMEOUT).ok().map(|stream| (stream, target)));
    let cancelled = shared.cancelled.lock().is_ok_and(|mut set| set.remove(&address));
    match stream {
        Some((stream, target)) if !cancelled && shared.round.load(Ordering::Relaxed) == started_in => {
            let refused = shared.no_control.clone();
            attach(stream, target, &shared, refused, Some(address));
        }
        _ => {
            let _ = shared.news.send(News::Failed { address });
        }
    }
}

/// Starts the two threads of a connection, or says that it failed. Running out of threads is
/// the one thing here a flood could still reach, and it must not end the listener with a panic.
fn attach(stream: TcpStream, far: SocketAddr, shared: &Shared, no_control: Arc<AtomicBool>, address: Option<String>) {
    let accepted = address.is_none();
    let failed = |address: Option<String>| {
        if let Some(address) = address {
            let _ = shared.news.send(News::Failed { address });
        }
    };
    let (Ok(mut writer), Ok(kept)) = (stream.try_clone(), stream.try_clone()) else {
        return failed(address);
    };
    let _ = stream.set_nodelay(true);
    let _ = stream.set_write_timeout(Some(WRITE_TIMEOUT));
    let _ = stream.set_read_timeout(Some(IDLE / 4));
    let (frames, to_send) = sync_channel(OUTBOX);
    let stats = Stats::new();
    let counted = stats.clone();
    if thread::Builder::new().spawn(move || write_frames(&mut writer, &to_send, &counted)).is_err() {
        return failed(address);
    }
    let peer = shared.next_peer.fetch_add(1, Ordering::Relaxed);
    let answers = frames.clone();
    if let Ok(mut peers) = shared.peers.lock() {
        peers.insert(peer, Peer { stream: kept, far, frames, accepted, stats: stats.clone() });
    }
    let _ = shared.news.send(News::Opened { number: peer, address });
    let reading = shared.clone();
    let end = move |shared: &Shared| {
        if let Ok(mut peers) = shared.peers.lock() {
            peers.remove(&peer);
        }
        let _ = shared.news.send(News::Closed { number: peer });
    };
    let spawned = thread::Builder::new().spawn(move || {
        let reader = Reader { peer, sender: &reading.sender, no_control: &no_control, stats: &stats, answers };
        read_messages(stream, &reader);
        end(&reading);
    });
    if spawned.is_err() {
        end(shared);
    }
}

/// Runs on its own thread, so a peer that reads slowly costs only itself. It ends when the peer
/// leaves the list.
fn write_frames(stream: &mut TcpStream, frames: &Receiver<Frame>, stats: &Stats) {
    for frame in frames {
        stats.sent.fetch_add(frame.len() as u64, Ordering::Relaxed);
        if stream.write_all(&frame).is_err() {
            // Half a frame may have left, so this connection can never be read again.
            let _ = stream.shutdown(Shutdown::Both);
            return;
        }
    }
}

fn is_control(message: &[u8]) -> bool {
    network::read(message).is_some_and(|(mark, _)| mark == network::CONTROL)
}

/// What the reading thread of one connection works with.
struct Reader<'a> {
    peer: u32,
    sender: &'a Inbox,
    no_control: &'a AtomicBool,
    stats: &'a Stats,
    /// The connection's own queue out, for pings and their answers.
    answers: SyncSender<Frame>,
}

fn read_messages(mut stream: TcpStream, reader: &Reader) {
    let Reader { peer, sender, no_control, stats, answers } = reader;
    let mut decoder = Decoder::default();
    let mut bytes = vec![0u8; READ_SIZE];
    let mut whole_ms = Instant::now();
    let mut pinged: Option<Instant> = None;
    loop {
        if whole_ms.elapsed() >= IDLE {
            return;
        }
        if pinged.is_none_or(|at| at.elapsed() >= PING_EVERY) {
            pinged = Some(Instant::now());
            let _ = answers.try_send(stats.ping().into());
        }
        let size = match stream.read(&mut bytes) {
            Ok(0) => return,
            Ok(size) => size,
            Err(error) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                continue;
            }
            Err(_) => return,
        };
        stats.received.fetch_add(size as u64, Ordering::Relaxed);
        decoder.push(&bytes[..size]);
        while let Some(message) = decoder.next_message() {
            whole_ms = Instant::now();
            if let Some(answer) = stats.heard(&message) {
                if let Some(answer) = answer {
                    let _ = answers.try_send(answer.into());
                }
                continue;
            }
            if no_control.load(Ordering::Relaxed) && is_control(&message) {
                continue;
            }
            if sender.send((*peer, message)).is_err() {
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
        let frame: Frame = encode(message).into();
        peers.retain(|number, peer| {
            let wanted = match target {
                Target::Peer(one) => *number == one,
                Target::All => true,
                Target::AllExcept(other) => *number != other,
            };
            if !wanted || peer.frames.try_send(frame.clone()).is_ok() {
                return true;
            }
            let _ = peer.stream.shutdown(Shutdown::Both);
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
        if let Some(peer) = self.shared.peers.lock().ok().and_then(|mut peers| peers.remove(&number)) {
            let _ = peer.stream.shutdown(Shutdown::Both);
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

    fn wait_for(what: &str, check: impl Fn() -> bool) {
        let until = Instant::now() + Duration::from_secs(5);
        while !check() {
            assert!(Instant::now() < until, "{what}");
            thread::sleep(Duration::from_millis(5));
        }
    }

    /// A socket's buffers fill and then a write waits. The node must never be the one waiting.
    #[test]
    fn a_peer_that_stops_reading_never_holds_up_the_node() {
        let mut link = TcpLink::new();
        let address = link.listen("127.0.0.1:0").unwrap();
        let control = link.control();
        let never_reads = TcpStream::connect(address).unwrap();
        wait_for("it connects", || control.count() == 1);

        let message = stamp(OPEN, &[7; 4_000]);
        let mut slowest = Duration::ZERO;
        for _ in 0..20_000 {
            let started = Instant::now();
            link.send(Target::All, &message);
            slowest = slowest.max(started.elapsed());
        }
        assert!(slowest < Duration::from_millis(500), "a send took {slowest:?}");
        wait_for("the peer goes", || control.count() == 0);
        drop(never_reads);
    }

    /// A person who stops letting others connect expects the door shut now, not at the next start.
    #[test]
    fn listening_elsewhere_shuts_the_old_door_and_drops_who_came_through_it() {
        let link = TcpLink::new();
        let first = link.listen("127.0.0.1:0").unwrap();
        let control = link.control();
        let guest = TcpStream::connect(first).unwrap();
        wait_for("the guest connects", || control.count() == 1);

        let second = control.listen_again("127.0.0.1:0").unwrap();
        wait_for("the guest goes", || control.count() == 0);
        assert!(TcpStream::connect(first).is_err(), "the old address still takes connections");
        let newcomer = TcpStream::connect(second).unwrap();
        wait_for("the new address takes one", || control.count() == 1);
        drop((guest, newcomer));
    }

    #[test]
    fn connections_from_outside_stop_at_the_limit_and_dials_still_go_out() {
        let mut link = TcpLink::new();
        let address = link.listen("127.0.0.1:0").unwrap();
        let held: Vec<_> =
            (0..MOST_ACCEPTED + 8).map(|_| TcpStream::connect(address).unwrap()).collect();
        thread::sleep(Duration::from_millis(300));
        assert_eq!(link.control().count(), MOST_ACCEPTED);

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let outside = listener.local_addr().unwrap().to_string();
        while link.news().is_some() {}
        Link::dial(&mut link, &outside);
        let (far_end, _) = listener.accept().unwrap();
        assert!(matches!(next_news(&mut link), News::Opened { address: Some(_), .. }));
        drop((held, far_end));
    }

    /// Memory stays flat when a node falls behind: the far end ends up waiting on its own socket.
    #[test]
    fn a_node_that_falls_behind_stops_taking_bytes() {
        let link = TcpLink::new();
        let address = link.listen("127.0.0.1:0").unwrap();
        let mut stranger = TcpStream::connect(address).unwrap();
        stranger.set_write_timeout(Some(Duration::from_secs(1))).unwrap();
        let frame = encode(&stamp(OPEN, &[1; 1_000]));
        let mut written = 0;
        while written < 256 * 1024 * 1024 && stranger.write_all(&frame).is_ok() {
            written += frame.len();
        }
        assert!(written < 64 * 1024 * 1024, "{written} bytes went in");
    }

    /// One byte now and then keeps a socket busy and a place taken, and brings nothing.
    #[test]
    fn a_connection_that_never_finishes_a_message_goes() {
        let link = TcpLink::new();
        let address = link.listen("127.0.0.1:0").unwrap();
        let control = link.control();
        let mut stranger = TcpStream::connect(address).unwrap();
        wait_for("it connects", || control.count() == 1);
        stranger.write_all(&100u32.to_le_bytes()).unwrap();
        let started = Instant::now();
        while control.count() == 1 {
            assert!(started.elapsed() < IDLE * 3, "it stayed");
            let _ = stranger.write_all(&[1]);
            thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn a_quiet_connection_goes_and_says_so() {
        let mut link = TcpLink::new();
        let address = link.listen("127.0.0.1:0").unwrap();
        let quiet = TcpStream::connect(address).unwrap();
        assert!(matches!(next_news(&mut link), News::Opened { address: None, .. }));
        assert!(matches!(next_news(&mut link), News::Closed { .. }));
        drop(quiet);
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
