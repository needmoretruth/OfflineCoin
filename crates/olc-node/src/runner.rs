//! Connects an [`Engine`] to any number of transports.
use crate::engine::{Engine, Outgoing, PeerId};
use crate::message::Message;
use alloc::boxed::Box;
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

pub struct Runner {
    pub engine: Engine,
    /// Cuts every link while true, so the chain can fork on purpose.
    pub offline: bool,
    links: Vec<Box<dyn Link>>,
}

impl Runner {
    pub fn new(engine: Engine) -> Self {
        Self {
            engine,
            offline: false,
            links: Vec::new(),
        }
    }

    pub fn add_link(&mut self, link: Box<dyn Link>) {
        self.links.push(link);
    }

    /// Peers heard recently through the link added at position `link`.
    pub fn peers_on(&self, link: usize) -> usize {
        self.engine
            .peer_ids()
            .filter(|peer| (peer >> LINK_SHIFT) as usize == link)
            .count()
    }

    /// Handles everything waiting on the links and sends everything the engine queued.
    pub fn step(&mut self, device_ms: u64) {
        self.engine.tick(device_ms);
        for (index, link) in self.links.iter_mut().enumerate() {
            while let Some((peer, bytes)) = link.receive() {
                if let (false, Some(message)) = (self.offline, Message::decode(&bytes)) {
                    let from: PeerId = (index as u32) << LINK_SHIFT | (peer & PEER_MASK);
                    self.engine.on_message(from, message);
                }
            }
        }
        while let Some(outgoing) = self.engine.take_outgoing() {
            if self.offline {
                continue;
            }
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
    }
}
