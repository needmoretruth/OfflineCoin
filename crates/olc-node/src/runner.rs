//! Connects an [`Engine`] to any number of transports.
use crate::engine::{Engine, Outgoing, PeerId};
use crate::message::Message;
use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::vec::Vec;

/// Who a [`Link`] should deliver to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Peer(u32),
    All,
    AllExcept(u32),
}

/// A transport: TCP, a serial cable, ESP-NOW, Bluetooth, a VPN, anything that carries bytes.
pub trait Link: Send {
    fn send(&mut self, target: Target, message: &[u8]);
    /// Returns the next whole message received, without blocking.
    fn receive(&mut self) -> Option<(u32, Vec<u8>)>;
}

/// A peer id holds the link index in its top byte, so a reply leaves through the right link.
const LINK_SHIFT: u32 = 24;
const PEER_MASK: u32 = (1 << LINK_SHIFT) - 1;
const MAX_CONTROL: usize = 32;

pub struct Runner {
    pub engine: Engine,
    /// Keeps the chain to itself while true, so it can fork on purpose. A board still answers
    /// control traffic, or nobody could bring it back from afar.
    pub offline: bool,
    links: Vec<Box<dyn Link>>,
    control: VecDeque<(PeerId, Vec<u8>)>,
}

impl Runner {
    pub fn new(engine: Engine) -> Self {
        Self {
            engine,
            offline: false,
            links: Vec::new(),
            control: VecDeque::new(),
        }
    }

    /// Returns the link's position, which is the top byte of its peers' ids.
    pub fn add_link(&mut self, link: Box<dyn Link>) -> usize {
        self.links.push(link);
        self.links.len() - 1
    }

    /// The position of the link that hears `peer`.
    pub fn link_of(peer: PeerId) -> usize {
        (peer >> LINK_SHIFT) as usize
    }

    /// Peers heard recently through the link added at position `link`.
    pub fn peers_on(&self, link: usize) -> usize {
        self.engine
            .peer_ids()
            .filter(|peer| (peer >> LINK_SHIFT) as usize == link)
            .count()
    }

    /// The next control packet heard, and who sent it.
    pub fn take_control(&mut self) -> Option<(PeerId, Vec<u8>)> {
        self.control.pop_front()
    }

    /// Sends now, whether or not the node is offline.
    pub fn send_now(&mut self, outgoing: Outgoing) {
        match outgoing {
            Outgoing::To(peer, message) => {
                if let Some(link) = self.links.get_mut((peer >> LINK_SHIFT) as usize) {
                    link.send(Target::Peer(peer & PEER_MASK), &message.encode());
                }
            }
            Outgoing::All { except, message } => {
                let bytes = message.encode();
                for (index, link) in self.links.iter_mut().enumerate() {
                    let target = match except {
                        Some(peer) if (peer >> LINK_SHIFT) as usize == index => {
                            Target::AllExcept(peer & PEER_MASK)
                        }
                        _ => Target::All,
                    };
                    link.send(target, &bytes);
                }
            }
        }
    }

    /// Handles everything waiting on the links and sends everything the engine queued.
    pub fn step(&mut self, device_ms: u64) {
        self.engine.tick(device_ms);
        for (index, link) in self.links.iter_mut().enumerate() {
            while let Some((peer, bytes)) = link.receive() {
                let from: PeerId = (index as u32) << LINK_SHIFT | (peer & PEER_MASK);
                match Message::decode(&bytes) {
                    Some(Message::Control(packet)) => {
                        if self.control.len() < MAX_CONTROL {
                            self.control.push_back((from, packet));
                        }
                    }
                    Some(message) if !self.offline => self.engine.on_message(from, message),
                    _ => {}
                }
            }
        }
        while let Some(outgoing) = self.engine.take_outgoing() {
            if self.offline {
                continue;
            }
            self.send_now(outgoing);
        }
    }
}
