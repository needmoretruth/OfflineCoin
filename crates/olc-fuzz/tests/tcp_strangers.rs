//! The scene a stranger makes: bytes thrown at a real node's real socket. The node has to carry on
//! and an honest peer has to get its blocks afterwards.
use olc_fuzz::fixture::grown_engine;
use olc_link_tcp::TcpLink;
use olc_node::frame::encode;
use olc_node::neighbours::HELLO_MS;
use olc_node::network::{mark_of, read, stamp, Mark};
use olc_node::{Heights, Link, Message, Runner, Target};
use std::io::Write;
use std::net::{SocketAddr, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

const STRANGERS: u64 = 8;
const THROWING: Duration = Duration::from_secs(3);
const BLOCKS: u64 = 4;
/// A node that stands still this long misses saying it is here, and its peers stop hearing it.
/// The longest wait between two turns is what shows the node carried on. A count of turns showed
/// the scheduler instead: a three-core macOS runner took about 60 turns a second against 450 on
/// Linux, most likely because the strangers' own threads held the cores and the node woke a
/// scheduler quantum late, each step taking under a millisecond, while a node that stood still
/// for 2.5 of the 3 seconds still took more than the 200 turns once asked for.
const LONGEST_STILL: Duration = Duration::from_millis(HELLO_MS);

/// One stranger's bytes: a real frame around garbage, half a frame, a false length, or noise.
fn thrown(random: &mut fastrand::Rng, mark: Mark) -> Vec<u8> {
    let body: Vec<u8> = (0..random.usize(..2_000)).map(|_| random.u8(..)).collect();
    match random.u8(..4) {
        0 => encode(&stamp(mark, &body)),
        1 => {
            let mut frame = encode(&stamp(mark, &body));
            frame.truncate(frame.len() / 2);
            frame
        }
        2 => (u32::MAX - 1).to_le_bytes().to_vec(),
        _ => body,
    }
}

fn throw_for(address: SocketAddr, seed: u64, mark: Mark, until: Instant) -> u64 {
    let mut random = fastrand::Rng::with_seed(seed);
    let mut sent = 0;
    while Instant::now() < until {
        let Ok(mut stream) = TcpStream::connect(address) else {
            continue;
        };
        for _ in 0..8 {
            let bytes = thrown(&mut random, mark);
            if stream.write_all(&bytes).is_err() {
                break;
            }
            sent += bytes.len() as u64;
        }
    }
    sent
}

#[test]
fn a_node_carries_on_after_strangers_throw_bytes_at_it() {
    let engine = grown_engine(1, BLOCKS);
    let genesis = engine.chain().genesis_id();
    let mark = mark_of(&genesis);
    let link = TcpLink::new();
    let address = link.listen("127.0.0.1:0").expect("a port");
    let mut runner = Runner::new(engine);
    runner.add_link(Box::new(link));

    let stop = Arc::new(AtomicBool::new(false));
    let turns = Arc::new(AtomicU64::new(0));
    let still_micros = Arc::new(AtomicU64::new(0));
    let (stopped, counted, still) = (stop.clone(), turns.clone(), still_micros.clone());
    let node = thread::spawn(move || {
        let mut now = 1_000_000;
        let mut last = Instant::now();
        while !stopped.load(Ordering::Relaxed) {
            now += 2;
            let waited = last.elapsed();
            last = Instant::now();
            still.fetch_max(u64::try_from(waited.as_micros()).unwrap_or(u64::MAX), Ordering::Relaxed);
            runner.step(now);
            counted.fetch_add(1, Ordering::Relaxed);
            thread::sleep(Duration::from_millis(2));
        }
    });

    let until = Instant::now() + THROWING;
    let strangers: Vec<_> = (0..STRANGERS)
        .map(|seed| thread::spawn(move || throw_for(address, seed, mark, until)))
        .collect();
    let sent: u64 = strangers.into_iter().map(|one| one.join().expect("a stranger")).sum();
    assert!(sent > 100_000, "the strangers threw only {sent} bytes");
    let turns_taken = turns.load(Ordering::Relaxed);
    let longest = Duration::from_micros(still_micros.load(Ordering::Relaxed));
    assert!(longest < LONGEST_STILL, "the node stood still {longest:?} in {turns_taken} turns under {sent} bytes");

    let mut honest = TcpLink::new();
    honest.dial(&address.to_string());
    let ask = Message::GetBlocks(vec![genesis], Heights(vec![0])).encode();
    let ask = stamp(mark, &ask);
    let until = Instant::now() + Duration::from_secs(10);
    let mut got = None;
    while got.is_none() && Instant::now() < until {
        honest.send(Target::All, &ask);
        thread::sleep(Duration::from_millis(50));
        while let Some((_, bytes)) = honest.receive() {
            if let Some(Message::Block(block)) = read(&bytes).and_then(|(_, body)| Message::decode(body)) {
                got = Some(block.id());
            }
        }
    }
    stop.store(true, Ordering::Relaxed);
    node.join().expect("the node did not end by a panic");
    assert!(got.is_some(), "an honest peer got no block after {sent} bytes of garbage");
}
