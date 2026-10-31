//! ESP-NOW transport. Messages are split into frames of at most 250 bytes.
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};
use esp_radio::esp_now::{EspNow, EspNowWifiInterface, PeerInfo, BROADCAST_ADDRESS};
use olc_node::fragment::{split, Reassembler};
use olc_node::{Link, Target};

const FRAME_SIZE: usize = 250;
const MAX_PEERS: usize = 16;

/// Turns ESP-NOW off without touching Wi-Fi, to prove the other links carry the chain alone.
pub static ENABLED: AtomicBool = AtomicBool::new(true);

pub struct RadioLink {
    radio: EspNow,
    peers: BTreeMap<u32, [u8; 6]>,
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
    pub fn new(radio: EspNow) -> Self {
        Self {
            radio,
            peers: BTreeMap::new(),
            joiner: Reassembler::default(),
            next_message: 0,
        }
    }

    /// Unicast frames are acknowledged and repeated by the radio, which broadcast frames are not.
    fn remember(&mut self, mac: [u8; 6]) {
        let id = peer_id(&mac);
        if self.peers.contains_key(&id) || self.peers.len() >= MAX_PEERS {
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

    fn send_frames(&mut self, to: [u8; 6], message: &[u8]) {
        self.next_message = self.next_message.wrapping_add(1);
        for frame in split(self.next_message, message, FRAME_SIZE) {
            if let Ok(waiter) = self.radio.send(&to, &frame) {
                let _ = waiter.wait();
            }
        }
    }
}

impl Link for RadioLink {
    fn send(&mut self, target: Target, message: &[u8]) {
        if !ENABLED.load(Ordering::Relaxed) {
            return;
        }
        let to = match target {
            Target::Peer(peer) => self.peers.get(&peer).copied().unwrap_or(BROADCAST_ADDRESS),
            Target::All | Target::AllExcept(_) => BROADCAST_ADDRESS,
        };
        self.send_frames(to, message);
    }

    fn receive(&mut self) -> Option<(u32, Vec<u8>)> {
        while let Some(received) = self.radio.receive() {
            if !ENABLED.load(Ordering::Relaxed) {
                continue;
            }
            let mac = received.info.src_address;
            self.remember(mac);
            let id = peer_id(&mac);
            if let Some(message) = self.joiner.push(id, received.data()) {
                return Some((id, message));
            }
        }
        None
    }
}
