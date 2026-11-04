//! The exploiter's side of one connection. It speaks to a node as any peer does, in the same
//! frames with the same network mark, and keeps its own copy of the node's chain from the blocks
//! the node serves, so what it builds stands on the node's real tip.
use olc_core::{AddOutcome, Chain, CoinConfig, Plugins};
use olc_pow::ProofOfWork;
use olc_node::frame::{encode, Decoder};
use olc_node::network::{self, Mark};
use olc_node::{Heights, Hello, Message};
use std::io::{ErrorKind, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

const DIAL: Duration = Duration::from_secs(4);
const WRITE: Duration = Duration::from_secs(4);
const HELLO_WAIT: Duration = Duration::from_secs(10);
/// A node answers a request for blocks at once when it is idle. On a computer busy with the Lab's
/// miners its first block may take a while; after one, this long without a block ends a round.
const QUIET: Duration = Duration::from_millis(400);
/// How long the first block of a round may take.
const FIRST_ANSWER: Duration = Duration::from_secs(5);
/// Blocks a node serves for one request (`olc_node::sync::BLOCKS_AT_ONCE`).
const BLOCKS_AT_ONCE: usize = 8;
/// Long enough for a thousand Lab blocks, a hundred and twenty-five rounds.
const SYNC_WAIT: Duration = Duration::from_secs(120);
/// How long two nodes may stand on different tips before a trick gives up on agreeing them; a
/// block reaches every Lab node in well under a second.
const TURN_WAIT: Duration = Duration::from_secs(15);
const TURN_LOOK: Duration = Duration::from_millis(300);
const READ_SIZE: usize = 16 * 1024;

/// The connection, closed when it goes, so the node's reader never waits on a socket left open.
struct Line(TcpStream);

impl Drop for Line {
    fn drop(&mut self) {
        let _ = self.0.shutdown(Shutdown::Both);
    }
}

pub(super) struct Peer {
    target: SocketAddr,
    line: Line,
    heard: Receiver<Message>,
    mark: Mark,
    pub(super) chain: Chain,
}

/// Reads whole messages off the socket until it closes. Anything not a message of this network or
/// an open one is dropped, as a node drops it.
fn read_messages(mut stream: TcpStream, mark: Mark, heard: Sender<Message>) {
    let mut decoder = Decoder::default();
    let mut bytes = vec![0u8; READ_SIZE];
    loop {
        let size = match stream.read(&mut bytes) {
            Ok(0) => return,
            Ok(size) => size,
            Err(error) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::Interrupted) => continue,
            Err(_) => return,
        };
        decoder.push(&bytes[..size]);
        if decoder.is_broken() {
            return;
        }
        while let Some(whole) = decoder.next_message() {
            let Some((said_mark, body)) = network::read(&whole) else {
                continue;
            };
            if said_mark != mark && said_mark != network::OPEN {
                continue;
            }
            if let Some(message) = Message::decode(body) {
                if heard.send(message).is_err() {
                    return;
                }
            }
        }
    }
}

fn dial(target: SocketAddr, mark: Mark) -> Result<(TcpStream, Receiver<Message>), String> {
    let stream = TcpStream::connect_timeout(&target, DIAL).map_err(|error| format!("{target}: {error}"))?;
    let _ = stream.set_nodelay(true);
    stream.set_write_timeout(Some(WRITE)).map_err(|error| error.to_string())?;
    let reading = stream.try_clone().map_err(|error| error.to_string())?;
    let (sender, heard) = channel();
    thread::Builder::new()
        .name("trouble-reader".into())
        .spawn(move || read_messages(reading, mark, sender))
        .map_err(|error| error.to_string())?;
    Ok((stream, heard))
}

impl Peer {
    /// Connects to `target` with the copy `kept` from a try before, or a new one that starts at
    /// the first block. Either way [`Peer::catch_up`] fetches only the blocks it lacks.
    pub(super) fn connect(coin: &CoinConfig, target: SocketAddr, kept: &mut Option<Chain>) -> Result<Self, String> {
        let chain = match kept.take() {
            Some(chain) => chain,
            // The plugins every node of this app runs with.
            None => Chain::new(coin.clone(), Plugins { consensus: Box::new(ProofOfWork), languages: olc_lang::all() }),
        };
        let mark = network::mark_of(&chain.genesis_id());
        match dial(target, mark) {
            Ok((stream, heard)) => Ok(Self { target, line: Line(stream), heard, mark, chain }),
            Err(error) => {
                *kept = Some(chain);
                Err(error)
            }
        }
    }

    /// The copy, for the next try.
    pub(super) fn into_chain(self) -> Chain {
        self.chain
    }

    pub(super) fn send(&mut self, message: &Message) -> Result<(), String> {
        let mark = match message {
            Message::Who | Message::Hello(_) => network::OPEN,
            _ => self.mark,
        };
        let frame = encode(&network::stamp(mark, &message.encode()));
        self.line.0.write_all(&frame).map_err(|error| format!("{}: {error}", self.target))
    }

    /// Asks the node what it is. A node drops a connection that said nothing for a while, so a
    /// closed one is dialled again once.
    pub(super) fn hello(&mut self) -> Result<Hello, String> {
        match self.ask_hello() {
            Ok(hello) => Ok(hello),
            Err(_) => {
                let (stream, heard) = dial(self.target, self.mark)?;
                (self.line, self.heard) = (Line(stream), heard);
                self.ask_hello()
            }
        }
    }

    fn ask_hello(&mut self) -> Result<Hello, String> {
        self.drain();
        self.send(&Message::Who)?;
        let until = Instant::now() + HELLO_WAIT;
        loop {
            let left = until.saturating_duration_since(Instant::now());
            match self.heard.recv_timeout(left) {
                Ok(Message::Hello(hello)) => return Ok(*hello),
                Ok(_) => {}
                Err(RecvTimeoutError::Timeout) => return Err(format!("{} did not say what it is.", self.target)),
                Err(RecvTimeoutError::Disconnected) => return Err(format!("{} closed the connection.", self.target)),
            }
        }
    }

    /// Drops what the node said so far: its announcements are not answers to anything here.
    fn drain(&mut self) {
        while self.heard.try_recv().is_ok() {}
    }

    /// Fetches blocks until this copy's tip is the node's, as a node catching up does.
    pub(super) fn catch_up(&mut self) -> Result<Hello, String> {
        let hello = self.hello()?;
        if hello.coin.message != self.chain.config().message || hello.coin.name != self.chain.config().name {
            return Err(format!("{} runs another coin: {}.", self.target, hello.coin.name));
        }
        let until = Instant::now() + SYNC_WAIT;
        while !self.chain.contains(&hello.tip) {
            let before = self.chain.tip();
            let (heights, ids): (Vec<u64>, Vec<_>) = self.chain.locator().into_iter().unzip();
            self.drain();
            self.send(&Message::GetBlocks(ids, Heights(heights)))?;
            let mut got = 0;
            let first_by = Instant::now() + FIRST_ANSWER;
            while got < BLOCKS_AT_ONCE && !self.chain.contains(&hello.tip) {
                let wait = if got == 0 { first_by.saturating_duration_since(Instant::now()) } else { QUIET };
                match self.heard.recv_timeout(wait) {
                    // A node sends blocks ahead of an ask, and those of an earlier ask may come only
                    // now, after the drain: one the copy holds already, or keeps as an orphan
                    // until its parent comes, moves it nowhere and ends no round.
                    Ok(Message::Block(block)) => {
                        if matches!(self.chain.add_block(block), Ok(AddOutcome::NewTip | AddOutcome::Reorganized { .. })) {
                            got += 1;
                        }
                    }
                    // A node that let these blocks go says so: no wait helps.
                    Ok(Message::Pruned { .. }) => break,
                    Ok(_) => {}
                    Err(RecvTimeoutError::Timeout) => break,
                    Err(RecvTimeoutError::Disconnected) => return Err(format!("{} closed the connection.", self.target)),
                }
            }
            if self.chain.tip() == before && !self.chain.contains(&hello.tip) {
                return Err(format!("{} served no blocks after height {}.", self.target, self.chain.height()));
            }
            if Instant::now() > until {
                return Err(format!("{} served its blocks too slowly.", self.target));
            }
        }
        // A copy kept from a try before may stand a block past this node, taken from the target.
        if self.chain.id_at(hello.height) != Some(hello.tip) {
            return Err(format!("{} follows a branch this copy does not.", self.target));
        }
        Ok(hello)
    }

    /// Keeps the copy and speaks to `target` from now on, once both stand on one tip. The target
    /// may keep only recent blocks, so it is asked for no more than the newest few the copy lacks;
    /// one a block behind is given a moment to hear of it from its neighbours.
    pub(super) fn turn_to(&mut self, target: SocketAddr) -> Result<Hello, String> {
        let (stream, heard) = dial(target, self.mark)?;
        (self.target, self.line, self.heard) = (target, Line(stream), heard);
        let until = Instant::now() + TURN_WAIT;
        loop {
            let hello = self.hello()?;
            if hello.tip == self.chain.tip() {
                return Ok(hello);
            }
            if !self.chain.contains(&hello.tip) {
                if let Ok(hello) = self.catch_up() {
                    return Ok(hello);
                }
            }
            if Instant::now() > until {
                return Err(format!("{target} stayed on another tip than the node the copy came from."));
            }
            thread::sleep(TURN_LOOK);
        }
    }
}
