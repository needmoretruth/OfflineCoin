//! A shout across the LAN, for what a firewall lets through. Some networks pass broadcasts and some
//! drop them, so this only adds to what the outward look finds.
use olc_node::network::{read, stamp, OPEN};
use olc_node::{Hello, Message};
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

const PORT: u16 = 7000;
const SHOUT_EVERY: Duration = Duration::from_secs(15);
const MAX_DATAGRAM: usize = 1_400;
/// Hellos waiting for the next round. Anyone on the network can send as many as they like.
const MOST_WAITING: usize = 64;

#[derive(Default)]
struct Shared {
    /// What this app says about itself.
    me: Mutex<Option<Hello>>,
    heard: Mutex<Vec<(SocketAddr, Hello)>>,
    stop: AtomicBool,
}

pub struct Shout {
    shared: Arc<Shared>,
}

fn datagram(message: &Message) -> Option<Vec<u8>> {
    let bytes = stamp(OPEN, &message.encode());
    (bytes.len() <= MAX_DATAGRAM).then_some(bytes)
}

impl Shout {
    pub fn start() -> Self {
        let shared = Arc::new(Shared::default());
        let listener = shared.clone();
        thread::spawn(move || listen(&listener));
        let speaker = shared.clone();
        thread::spawn(move || speak(&speaker));
        Self { shared }
    }

    pub fn set_me(&self, me: Option<Hello>) {
        if let Ok(mut slot) = self.shared.me.lock() {
            *slot = me;
        }
    }

    pub fn take_heard(&self) -> Vec<(SocketAddr, Hello)> {
        self.shared
            .heard
            .lock()
            .map(|mut heard| heard.drain(..).collect())
            .unwrap_or_default()
    }
}

impl Drop for Shout {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
    }
}

fn speak(shared: &Shared) {
    let Ok(socket) = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)) else {
        return;
    };
    if socket.set_broadcast(true).is_err() {
        return;
    }
    while !shared.stop.load(Ordering::Relaxed) {
        let me = shared.me.lock().ok().and_then(|me| me.clone());
        if let Some(bytes) = me.and_then(|me| datagram(&Message::Hello(Box::new(me)))) {
            let _ = socket.send_to(&bytes, (Ipv4Addr::BROADCAST, PORT));
        }
        thread::sleep(SHOUT_EVERY);
    }
}

fn listen(shared: &Shared) {
    // A second app on this computer has the port already and only speaks.
    let Ok(socket) = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, PORT)) else {
        return;
    };
    let _ = socket.set_read_timeout(Some(SHOUT_EVERY));
    let mut bytes = vec![0u8; MAX_DATAGRAM];
    while !shared.stop.load(Ordering::Relaxed) {
        let Ok((size, from)) = socket.recv_from(&mut bytes) else {
            continue;
        };
        let Some((mark, body)) = read(&bytes[..size]) else {
            continue;
        };
        if mark != OPEN {
            continue;
        }
        if let Some(Message::Hello(hello)) = Message::decode(body) {
            if let Ok(mut heard) = shared.heard.lock() {
                if heard.len() < MOST_WAITING {
                    heard.push((from, *hello));
                }
            }
        }
    }
}
