//! Connects an [`Engine`] to any number of transports.
use crate::dial::{Dialer, Order};
use crate::engine::{Engine, PeerId};
use crate::event::Outgoing;
use crate::hello::{Card, Hello};
use crate::medium::{Medium, Rank};
use crate::message::Message;
use crate::neighbours::{Fan, Move, Neighbour, Neighbours, HELLO_MS};
use crate::network::{self, Mark};
use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::vec::Vec;

/// Who a [`Link`] should deliver to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Peer(u32),
    All,
    AllExcept(u32),
}

/// What a link that must connect has to tell. A link with no connections has nothing to say.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum News {
    /// A connection opened. `address` is the dial it answers, if this end dialled.
    Opened { number: u32, address: Option<String> },
    Closed { number: u32 },
    /// A dial ended without a connection.
    Failed { address: String },
    /// A node showed itself at `address`, and could be dialled there.
    Seen { node: u64, address: String },
}

/// A transport: TCP, a serial cable, ESP-NOW, Bluetooth, a VPN, anything that carries bytes.
pub trait Link: Send {
    fn medium(&self) -> Medium;
    fn send(&mut self, target: Target, message: &[u8]);
    /// Returns the next whole message received, without blocking.
    fn receive(&mut self) -> Option<(u32, Vec<u8>)>;
    /// Starts connecting to `address`, and returns at once.
    fn dial(&mut self, _address: &str) {}
    /// Gives up a dial that has not finished.
    fn cancel(&mut self, _address: &str) {}
    /// Ends a connection that has gone silent, so its address can be dialled again.
    fn close(&mut self, _number: u32) {}
    fn news(&mut self) -> Option<News> {
        None
    }
}

/// A peer id holds the link index in its top byte, so a reply leaves through the right link.
const LINK_SHIFT: u32 = 24;
const PEER_MASK: u32 = (1 << LINK_SHIFT) - 1;
const MAX_CONTROL: usize = 32;
const MAX_HELLOS: usize = 32;
const MAX_MOVES: usize = 16;
/// Messages taken from one link in a turn. A node that falls behind on a flood must still mine,
/// serve its other links and feed a board's watchdog, and the rest waits on the link.
const MOST_HEARD_PER_TURN: usize = 64;

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
    /// The id this node goes by, whichever link it is heard on.
    me: u64,
    table: Neighbours,
    dialer: Dialer,
    hello_ms: u64,
    moves: VecDeque<Move>,
    /// Nodes this one acts as if it cannot hear, on every link. A test lays real nodes out in a
    /// line on one desk this way.
    ignores: fn(u64) -> bool,
}

fn hears_everyone(_node: u64) -> bool {
    false
}

fn id_of_address(address: &[u8]) -> u64 {
    let mut bytes = [0u8; 8];
    for (to, from) in bytes.iter_mut().zip(address) {
        *to = *from;
    }
    u64::from_le_bytes(bytes)
}

impl Runner {
    pub fn new(engine: Engine) -> Self {
        Self {
            mark: network::mark_of(&engine.chain().genesis_id()),
            me: id_of_address(&engine.address()),
            engine,
            offline: false,
            links: Vec::new(),
            control: VecDeque::new(),
            card: Card::default(),
            hellos: VecDeque::new(),
            table: Neighbours::default(),
            dialer: Dialer::default(),
            hello_ms: 0,
            moves: VecDeque::new(),
            ignores: hears_everyone,
        }
    }

    /// Chooses which nodes this one acts as if it cannot hear. A neighbour stays known by name,
    /// so whatever link it moves to, its chain traffic is dropped unread and it is never dialled.
    pub fn set_ignores(&mut self, ignores: fn(u64) -> bool) {
        self.ignores = ignores;
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
        self.table.add_link(link.medium());
        self.links.push(link);
        self.links.len() - 1
    }

    /// Chooses the id this node goes by. Two nodes sharing a wallet need different ones, so a
    /// node with a machine of its own uses something only that machine has.
    pub fn set_id(&mut self, me: u64) {
        self.me = me;
    }

    pub fn id(&self) -> u64 {
        self.me
    }

    /// The order links are preferred in. Traffic follows it as soon as a better link is steady.
    /// Moves traffic off a link that was just turned off, without waiting for it to go quiet.
    pub fn link_off(&mut self, medium: Medium) {
        self.table.lost_link(medium);
    }

    pub fn set_rank(&mut self, rank: Rank) {
        self.table.rank = rank;
    }

    pub fn rank(&self) -> Rank {
        self.table.rank
    }

    /// Who is around.
    pub fn neighbours(&self) -> &[Neighbour] {
        self.table.list()
    }

    /// What kind of link the one at position `link` is.
    pub fn medium_of(&self, link: usize) -> Option<Medium> {
        self.table.medium(link)
    }

    /// The medium a neighbour heard as `peer` on a link is reached by now.
    pub fn medium_for(&self, peer: PeerId) -> Option<Medium> {
        let found = self.table.peer_of(Self::link_of(peer), Self::number_on_link(peer))?;
        self.table.medium_of(found)
    }

    /// Neighbours with a link that works, each counted once.
    pub fn neighbour_count(&self) -> usize {
        self.table.count()
    }

    /// Keeps a way to reach a node. It is dialled when nothing else reaches that node.
    pub fn add_address(&mut self, link: usize, address: String) {
        self.dialer.add(link, address);
    }

    /// Drops every address given so far.
    pub fn forget_addresses(&mut self) {
        self.dialer.clear();
    }

    /// Dials started since this node began.
    pub fn dials(&self) -> u64 {
        self.dialer.dials
    }

    /// The next move of traffic from one link to another.
    pub fn take_move(&mut self) -> Option<Move> {
        self.moves.pop_front()
    }

    /// The position of the link that hears `peer`.
    pub fn link_of(peer: PeerId) -> usize {
        (peer >> LINK_SHIFT) as usize
    }

    /// A peer's number inside its link.
    pub fn number_on_link(peer: PeerId) -> u32 {
        peer & PEER_MASK
    }

    /// Neighbours whose traffic goes through the link added at position `link`.
    pub fn peers_on(&self, link: usize) -> usize {
        self.table.count_on(link)
    }

    /// The next control packet heard, and who sent it.
    pub fn take_control(&mut self) -> Option<(PeerId, Vec<u8>)> {
        self.control.pop_front()
    }

    /// The next node that said what it is, and who it was.
    pub fn take_hello(&mut self) -> Option<(PeerId, Hello)> {
        self.hellos.pop_front()
    }

    /// Passes an answer heard on one link to the peers of another. A board behind a USB board has
    /// no other way to reach the computer.
    pub fn pass_hello(&mut self, link: usize, from: PeerId, mut hello: Hello) {
        hello.card.via = self.medium_for(from).map(Medium::name).unwrap_or_default().into();
        let bytes = self.stamped(&Message::Hello(hello.into()));
        if let Some(link) = self.links.get_mut(link) {
            link.send(Target::All, &bytes);
        }
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
        let status = self.engine.status();
        Hello {
            coin: self.engine.chain().config().clone(),
            tip: status.tip,
            height: status.height,
            work: status.work,
            address: self.engine.address(),
            card: Card { keeps: self.engine.keeps(), ..self.card.clone() },
        }
    }

    /// Where a peer id leads now. A neighbour's traffic follows its active link, and so does an
    /// answer meant for the link and number it was heard on.
    fn route(&self, peer: PeerId) -> Option<(usize, u32)> {
        let (link, number) = (Self::link_of(peer), Self::number_on_link(peer));
        if Neighbours::is_neighbour(peer) {
            return self.table.route(peer);
        }
        let moved = self.table.peer_of(link, number).and_then(|found| self.table.route(found));
        Some(moved.unwrap_or((link, number)))
    }

    /// Sends now, whether or not the node is offline.
    pub fn send_now(&mut self, outgoing: Outgoing) {
        match outgoing {
            Outgoing::To(peer, message) => {
                let bytes = self.stamped(&message);
                if let Some((link, number)) = self.route(peer) {
                    if let Some(link) = self.links.get_mut(link) {
                        link.send(Target::Peer(number), &bytes);
                    }
                }
            }
            Outgoing::All { except, message } => {
                let bytes = self.stamped(&message);
                let named = |peer: PeerId| {
                    let known = self.table.peer_of(Self::link_of(peer), Self::number_on_link(peer));
                    known.unwrap_or(peer)
                };
                let except = except.map(|peer| match Neighbours::is_neighbour(peer) {
                    true => peer,
                    false => named(peer),
                });
                for (index, link) in self.links.iter_mut().enumerate() {
                    match self.table.fan(index, except) {
                        Fan::Nobody => {}
                        Fan::Everyone => link.send(Target::All, &bytes),
                        Fan::EveryoneBut(number) => link.send(Target::AllExcept(number), &bytes),
                        Fan::Each(numbers) => {
                            for number in numbers {
                                link.send(Target::Peer(number), &bytes);
                            }
                        }
                    }
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

    fn here(&self) -> Vec<u8> {
        network::stamp(network::OPEN, &Message::Here(self.me).encode())
    }

    /// What the links that must connect have to say, and what to tell them.
    fn keep_connections(&mut self, now: u64) {
        let here = self.here();
        for (index, link) in self.links.iter_mut().enumerate() {
            while let Some(news) = link.news() {
                match news {
                    News::Opened { number, address } => {
                        if let Some(address) = address {
                            for order in self.dialer.opened(index, &address, number, now) {
                                if let Order::Cancel { address, .. } = order {
                                    link.cancel(&address);
                                }
                            }
                        }
                        // Neither end waits for the next Hello to know who the other is.
                        link.send(Target::Peer(number), &here);
                    }
                    News::Closed { number } => {
                        self.table.lost(index, number);
                        self.dialer.closed(index, number);
                    }
                    News::Failed { address } => self.dialer.failed(index, &address, now),
                    News::Seen { node, address } => self.dialer.seen(index, address, node, now),
                }
            }
        }
        for found in self.table.settle(now) {
            if self.moves.len() >= MAX_MOVES {
                self.moves.pop_front();
            }
            self.moves.push_back(found);
        }
        for order in self.dialer.step(&self.table, self.me, now) {
            match order {
                Order::Dial { link, address } => {
                    if let Some(link) = self.links.get_mut(link) {
                        link.dial(&address);
                    }
                }
                Order::Cancel { link, address } => {
                    if let Some(link) = self.links.get_mut(link) {
                        link.cancel(&address);
                    }
                }
                Order::Close { link, number } => {
                    if let Some(link) = self.links.get_mut(link) {
                        link.close(number);
                    }
                }
            }
        }
    }

    /// Says on every link who this node is, once a second.
    fn say_here(&mut self, now: u64) {
        // A node keeping to itself stays out of sight.
        if self.offline || now < self.hello_ms {
            return;
        }
        self.hello_ms = now + HELLO_MS;
        let here = self.here();
        for link in &mut self.links {
            link.send(Target::All, &here);
        }
    }

    /// Handles everything waiting on the links and sends everything the engine queued.
    pub fn step(&mut self, device_ms: u64) {
        self.engine.tick(device_ms);
        let mut asked = Vec::new();
        for (index, link) in self.links.iter_mut().enumerate() {
            for _ in 0..MOST_HEARD_PER_TURN {
                let Some((peer, bytes)) = link.receive() else {
                    break;
                };
                let number = peer & PEER_MASK;
                let from: PeerId = (index as u32) << LINK_SHIFT | number;
                let Some((mark, body)) = network::read(&bytes) else {
                    continue;
                };
                if mark == network::CONTROL {
                    self.table.hear(index, number, device_ms);
                    if let (Some(Message::Control(packet)), true) =
                        (Message::decode(body), self.control.len() < MAX_CONTROL)
                    {
                        self.control.push_back((from, packet));
                    }
                } else if mark == network::OPEN {
                    match Message::decode(body) {
                        Some(Message::Here(node)) if node == self.me => continue,
                        Some(Message::Here(node)) => {
                            if let Some(gone) = self.table.name(index, number, node, device_ms) {
                                self.engine.forget_peer(gone);
                            }
                            if (self.ignores)(node) {
                                if let Some(peer) = self.table.peer_on(index, number) {
                                    self.engine.forget_peer(peer);
                                }
                            } else {
                                self.dialer.learn(index, number, node);
                            }
                            continue;
                        }
                        _ if self.table.node_on(index, number).is_some_and(self.ignores) => continue,
                        Some(Message::Who) => asked.push(from),
                        Some(Message::Hello(hello)) if self.hellos.len() < MAX_HELLOS => {
                            self.hellos.push_back((from, *hello));
                        }
                        _ => {}
                    }
                    self.table.hear(index, number, device_ms);
                } else if mark == self.mark && !self.offline 
                    && !self.table.node_on(index, number).is_some_and(self.ignores)
                {
                    let neighbour = self.table.hear(index, number, device_ms);
                    if let Some(message) = Message::decode(body) {
                        self.engine.on_message(neighbour, message);
                    }
                }
            }
        }
        self.keep_connections(device_ms);
        self.say_here(device_ms);
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
