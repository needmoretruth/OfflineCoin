//! ESP-NOW transport. Messages are split into frames of at most 250 bytes.
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use crate::node::device_ms;
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, Ordering};
use esp_radio::esp_now::{EspNow, EspNowWifiInterface, PeerInfo, BROADCAST_ADDRESS};
use olc_node::fragment::{split, Reassembler};
use olc_node::network::{self, Mark};
use olc_node::{Link, Medium, Target};

const FRAME_SIZE: usize = 250;
/// What one frame to a known board holds. Everyone runs the same radio library, whose frames
/// reach this size, so a board of any build reads them.
const LARGE_FRAME_SIZE: usize = 1_470;
const MAX_PEERS: usize = 16;
/// Boards of another coin reach this board by control traffic alone, and may hold only this many
/// entries, so the table always has room for this network.
const MAX_OTHER_PEERS: usize = 8;
/// A neighbour that did not acknowledge gets broadcasts this long. The radio repeats a frame
/// nobody acknowledges many times while the node waits, and a Wi-Fi card that only listens to
/// the air, as a computer's does, never acknowledges anything.
const DEAF_MS: u64 = 30_000;

/// Turns ESP-NOW off without touching Wi-Fi, to prove the other links carry the chain alone.
pub static ENABLED: AtomicBool = AtomicBool::new(true);

/// How strongly the last frame arrived, in dBm. It tells how much reach the power leaves.
pub static LAST_RSSI: AtomicI32 = AtomicI32::new(0);

const NOBODY: u32 = u32::MAX;
/// Boards this one acts as if it cannot hear, by peer id. A test can lay real boards out in a line
/// on one desk this way. Nothing here survives a restart.
static IGNORED: [AtomicU32; 4] = [const { AtomicU32::new(NOBODY) }; 4];

/// Stops hearing a board, by the end of its MAC address such as `AAEF90`.
pub fn ignore(peer: u32) -> bool {
    let free = IGNORED
        .iter()
        .find(|slot| slot.load(Ordering::Relaxed) == NOBODY);
    free.inspect(|slot| slot.store(peer, Ordering::Relaxed))
        .is_some()
}

pub fn hear_everyone() {
    for slot in &IGNORED {
        slot.store(NOBODY, Ordering::Relaxed);
    }
}

/// Whether a node, by the id it goes by, is a board this one acts as if it cannot hear. A board
/// goes by its radio address, so the wire, Bluetooth and Wi-Fi go quiet for it as ESP-NOW does.
pub fn ignores_node(node: u64) -> bool {
    let id = node.to_le_bytes();
    is_ignored(peer_id(&[id[0], id[1], id[2], id[3], id[4], id[5]]))
}

fn is_ignored(peer: u32) -> bool {
    IGNORED
        .iter()
        .any(|slot| slot.load(Ordering::Relaxed) == peer)
}

pub struct RadioLink {
    radio: EspNow,
    mark: Mark,
    peers: BTreeMap<u32, [u8; 6]>,
    /// Who did not acknowledge, and until when to leave them out of unicast.
    deaf: BTreeMap<u32, u64>,
    joiner: Reassembler,
    next_message: u16,
}

// SAFETY: the firmware has one task and `RadioLink` never leaves it.
// `Link` asks for `Send` because computers share their node between threads.
unsafe impl Send for RadioLink {}

/// The peer id is the last three bytes of the sender's MAC address.
fn peer_id(mac: &[u8; 6]) -> u32 {
    u32::from_be_bytes([0, mac[3], mac[4], mac[5]])
}

impl RadioLink {
    pub fn new(radio: EspNow, mark: Mark) -> Self {
        Self {
            radio,
            mark,
            peers: BTreeMap::new(),
            deaf: BTreeMap::new(),
            joiner: Reassembler::default(),
            next_message: 0,
        }
    }

    /// Unicast frames are acknowledged and repeated by the radio, which broadcast frames are not.
    fn remember(&mut self, mac: [u8; 6], limit: usize) {
        let id = peer_id(&mac);
        if self.peers.contains_key(&id) || self.peers.len() >= limit {
            return;
        }
        let peer = PeerInfo {
            interface: EspNowWifiInterface::Station,
            peer_address: mac,
            lmk: None,
            channel: None,
            encrypt: false,
        };
        if self.radio.add_peer(peer).is_ok() {
            self.peers.insert(id, mac);
        }
    }

    /// Says whether every frame went. The first that did not ends the message.
    fn send_frames(&mut self, to: [u8; 6], message: &[u8]) -> bool {
        self.next_message = self.next_message.wrapping_add(1);
        let size = if to == BROADCAST_ADDRESS {
            FRAME_SIZE
        } else {
            LARGE_FRAME_SIZE
        };
        for frame in split(self.next_message, message, size) {
            let sent = self.radio.send(&to, &frame).map(|waiter| waiter.wait());
            if !matches!(sent, Ok(Ok(()))) {
                return false;
            }
        }
        true
    }
}

impl Link for RadioLink {
    fn medium(&self) -> Medium {
        Medium::EspNow
    }

    fn send(&mut self, target: Target, message: &[u8]) {
        if !ENABLED.load(Ordering::Relaxed) {
            return;
        }
        let now = device_ms();
        let hearing = |peer: &u32| self.deaf.get(peer).is_none_or(|until| now >= *until);
        let chosen = match target {
            Target::Peer(peer) if hearing(&peer) => self.peers.get(&peer).map(|mac| (peer, *mac)),
            _ => None,
        };
        if let Some((peer, mac)) = chosen {
            if self.send_frames(mac, message) {
                return;
            }
            self.deaf.insert(peer, now + DEAF_MS);
        }
        self.send_frames(BROADCAST_ADDRESS, message);
    }

    fn receive(&mut self) -> Option<(u32, Vec<u8>)> {
        while let Some(received) = self.radio.receive() {
            if !ENABLED.load(Ordering::Relaxed) {
                continue;
            }
            let mac = received.info.src_address;
            if is_ignored(peer_id(&mac)) {
                continue;
            }
            LAST_RSSI.store(received.info.rx_control.rssi, Ordering::Relaxed);
            match network::read(received.data()) {
                Some((mark, _)) if mark == self.mark => self.remember(mac, MAX_PEERS),
                Some((network::CONTROL | network::OPEN, _)) => self.remember(mac, MAX_OTHER_PEERS),
                _ => continue,
            }
            let id = peer_id(&mac);
            if let Some(message) = self.joiner.push(id, received.data()) {
                return Some((id, message));
            }
        }
        None
    }
}
