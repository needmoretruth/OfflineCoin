//! Connects an [`Engine`] to any number of transports.
use crate::engine::{Engine, PeerId};
use crate::event::Outgoing;
use crate::hello::{Card, Hello};
use crate::message::Message;
use crate::network::{self, Mark};
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
const MAX_HELLOS: usize = 32;

pub struct Runner {
    pub engine: Engine,
    /// Keeps the chain to itself while true, so it can fork on purpose. A board still answers
    /// control traffic, or nobody could bring it back from afar.
    pub offline: bool,
    mark: Mark,
    links: Vec<Box<dyn Link>>,
    control: VecDeque<(PeerId, Vec<u8>)>,
    /// What this node says about itself. Its host keeps it current.
    pub card: Card,
    hellos: VecDeque<(PeerId, Hello)>,
}

impl Runner {
    pub fn new(engine: Engine) -> Self {
        Self {
            mark: network::mark_of(&engine.chain().genesis_id()),
            engine,
            offline: false,
            links: Vec::new(),
            control: VecDeque::new(),
            card: Card::default(),
            hellos: VecDeque::new(),
        }
    }

    /// Moves this node to another coin. Links stay open and traffic of the old coin is dropped
    /// unread from now on.
    pub fn set_engine(&mut self, engine: Engine) {
        self.mark = network::mark_of(&engine.chain().genesis_id());
        self.engine = engine;
    }

    /// What this node's messages start with. A link that cuts messages up needs it to drop other
    /// networks' frames before it keeps anything about them.
    pub fn mark(&self) -> Mark {
        self.mark
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

    /// A peer's number inside its link.
    pub fn number_on_link(peer: PeerId) -> u32 {
        peer & PEER_MASK
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

    /// The next node that said what it is, and who it was.
    pub fn take_hello(&mut self) -> Option<(PeerId, Hello)> {
        self.hellos.pop_front()
    }

    /// Asks every peer of every link what it is.
    pub fn ask_who(&mut self) {
        self.send_now(Outgoing::All {
            except: None,
            message: Message::Who,
        });
    }

    /// Asks only the peers on one link. Radio time is too dear to spend on a fresh table.
    pub fn ask_who_on(&mut self, link: usize) {
        let bytes = self.stamped(&Message::Who);
        if let Some(link) = self.links.get_mut(link) {
            link.send(Target::All, &bytes);
        }
    }

    fn hello(&self) -> Hello {
        let chain = self.engine.chain();
        Hello {
            coin: chain.config().clone(),
            tip: chain.tip(),
            height: chain.height(),
            work: chain.total_work(),
            address: self.engine.address(),
            card: self.card.clone(),
        }
    }

    /// Sends now, whether or not the node is offline.
    pub fn send_now(&mut self, outgoing: Outgoing) {
        match outgoing {
            Outgoing::To(peer, message) => {
                let bytes = self.stamped(&message);
                if let Some(link) = self.links.get_mut((peer >> LINK_SHIFT) as usize) {
                    link.send(Target::Peer(peer & PEER_MASK), &bytes);
                }
            }
            Outgoing::All { except, message } => {
                let bytes = self.stamped(&message);
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

    fn stamped(&self, message: &Message) -> Vec<u8> {
        let mark = match message {
            Message::Control(_) => network::CONTROL,
            Message::Who | Message::Hello(_) => network::OPEN,
            _ => self.mark,
        };
        network::stamp(mark, &message.encode())
    }

    /// Handles everything waiting on the links and sends everything the engine queued.
    pub fn step(&mut self, device_ms: u64) {
        self.engine.tick(device_ms);
        let mut asked = Vec::new();
        for (index, link) in self.links.iter_mut().enumerate() {
            while let Some((peer, bytes)) = link.receive() {
                let from: PeerId = (index as u32) << LINK_SHIFT | (peer & PEER_MASK);
                let Some((mark, body)) = network::read(&bytes) else {
                    continue;
                };
                if mark == network::CONTROL {
                    if let (Some(Message::Control(packet)), true) =
                        (Message::decode(body), self.control.len() < MAX_CONTROL)
                    {
                        self.control.push_back((from, packet));
                    }
                } else if mark == network::OPEN {
                    match Message::decode(body) {
                        Some(Message::Who) => asked.push(from),
                        Some(Message::Hello(hello)) if self.hellos.len() < MAX_HELLOS => {
                            self.hellos.push_back((from, *hello));
                        }
                        _ => {}
                    }
                } else if mark == self.mark && !self.offline {
                    if let Some(message) = Message::decode(body) {
                        self.engine.on_message(from, message);
                    }
                }
            }
        }
        // A node keeping to itself stays out of sight.
        if !self.offline {
            for peer in asked {
                self.send_now(Outgoing::To(peer, Message::Hello(self.hello().into())));
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
