//! The exploiter's side of one connection. It speaks to a node as any peer does, in the same
//! frames with the same network mark, and keeps its own copy of the node's chain from the blocks
//! the node serves, so what it builds stands on the node's real tip.
use olc_core::{Chain, CoinConfig, Plugins};
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
/// A node answers a request for blocks at once. This long without a block ends one round.
const QUIET: Duration = Duration::from_millis(400);
/// Blocks a node serves for one request (`olc_node::sync::BLOCKS_AT_ONCE`).
const BLOCKS_AT_ONCE: usize = 8;
/// Long enough for a thousand Lab blocks, a hundred and twenty-five rounds.
const SYNC_WAIT: Duration = Duration::from_secs(120);
const READ_SIZE: usize = 16 * 1024;

pub(super) struct Peer {
    target: SocketAddr,
    stream: TcpStream,
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
    /// Connects to `target` and fetches its chain from its first block on.
    pub(super) fn connect(coin: &CoinConfig, target: SocketAddr) -> Result<Self, String> {
        // The plugins every node of this app runs with.
        let plugins = Plugins { consensus: Box::new(ProofOfWork), languages: olc_lang::all() };
        let chain = Chain::new(coin.clone(), plugins);
        let mark = network::mark_of(&chain.genesis_id());
        let (stream, heard) = dial(target, mark)?;
        Ok(Self { target, stream, heard, mark, chain })
    }

    pub(super) fn send(&mut self, message: &Message) -> Result<(), String> {
        let mark = match message {
            Message::Who | Message::Hello(_) => network::OPEN,
            _ => self.mark,
        };
        let frame = encode(&network::stamp(mark, &message.encode()));
        self.stream.write_all(&frame).map_err(|error| format!("{}: {error}", self.target))
    }

    /// Asks the node what it is. A node drops a connection that said nothing for a while, so a
    /// closed one is dialled again once.
    pub(super) fn hello(&mut self) -> Result<Hello, String> {
        match self.ask_hello() {
            Ok(hello) => Ok(hello),
            Err(_) => {
                let _ = self.stream.shutdown(Shutdown::Both);
                let (stream, heard) = dial(self.target, self.mark)?;
                (self.stream, self.heard) = (stream, heard);
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
            while got < BLOCKS_AT_ONCE && !self.chain.contains(&hello.tip) {
                match self.heard.recv_timeout(QUIET) {
                    Ok(Message::Block(block)) => {
                        got += 1;
                        let _ = self.chain.add_block(block);
                    }
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
        if self.chain.tip() != hello.tip {
            return Err(format!("{} follows a branch this copy does not.", self.target));
        }
        Ok(hello)
    }
}

impl Drop for Peer {
    fn drop(&mut self) {
        let _ = self.stream.shutdown(Shutdown::Both);
    }
}
