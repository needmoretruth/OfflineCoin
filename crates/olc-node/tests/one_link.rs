//! Two nodes joined by more than one link: one link in use, moving when it dies, coming back when
//! the better one has been steady, and no dialling while connected.
mod common;

use common::{engine, mine};
use olc_node::neighbours::{HELLO_MS, SILENT_MS, STEADY_MS};
use olc_node::{network, Link, Medium, Message, News, Runner, Target};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

/// One end of a pair of queues, with a switch that cuts the link both ways.
struct Wire {
    medium: Medium,
    inbox: Queue,
    outbox: Queue,
    cut: Arc<Mutex<bool>>,
    /// What came in on this link and was meant for the chain, in order.
    heard: Log,
    news: VecDeque<News>,
}

type Queue = Arc<Mutex<VecDeque<Vec<u8>>>>;
type Log = Arc<Mutex<Vec<Vec<u8>>>>;

impl Link for Wire {
    fn medium(&self) -> Medium {
        self.medium
    }

    fn send(&mut self, _target: Target, message: &[u8]) {
        if !*self.cut.lock().unwrap() {
            self.outbox.lock().unwrap().push_back(message.to_vec());
        }
    }

    fn receive(&mut self) -> Option<(u32, Vec<u8>)> {
        let message = self.inbox.lock().unwrap().pop_front()?;
        if *self.cut.lock().unwrap() {
            return None;
        }
        if is_chain_traffic(&message) {
            self.heard.lock().unwrap().push(message.clone());
        }
        Some((0, message))
    }

    /// The far end answers, as a real neighbour does.
    fn dial(&mut self, address: &str) {
        self.news.push_back(News::Opened { number: 0, address: Some(address.into()) });
    }

    fn news(&mut self) -> Option<News> {
        self.news.pop_front()
    }
}

fn is_chain_traffic(bytes: &[u8]) -> bool {
    let Some((_, body)) = network::read(bytes) else {
        return false;
    };
    let message = Message::decode(body);
    !matches!(message, None | Some(Message::Who | Message::Hello(_) | Message::Here(_)))
}

struct Handles {
    cut: Arc<Mutex<bool>>,
    /// What node B heard on this link.
    heard: Log,
}

fn pair(left: &mut Runner, right: &mut Runner, medium: Medium) -> Handles {
    let (one, two) = (Queue::default(), Queue::default());
    let cut = Arc::new(Mutex::new(false));
    let heard = Log::default();
    let wire = |inbox: &Queue, outbox: &Queue, heard: &Log| Wire {
        medium,
        inbox: inbox.clone(),
        outbox: outbox.clone(),
        cut: cut.clone(),
        heard: heard.clone(),
        news: VecDeque::new(),
    };
    left.add_link(Box::new(wire(&one, &two, &Log::default())));
    right.add_link(Box::new(wire(&two, &one, &heard)));
    Handles { cut, heard }
}

fn nodes() -> (Runner, Runner) {
    let (mut left, mut right) = (Runner::new(engine(1)), Runner::new(engine(2)));
    left.set_id(1);
    right.set_id(2);
    (left, right)
}

/// Steps both nodes through time, a tenth of a Hello at a time.
fn run(left: &mut Runner, right: &mut Runner, from: u64, to: u64) {
    for now in (from..to).step_by((HELLO_MS / 10) as usize) {
        for _ in 0..3 {
            left.step(now);
            right.step(now);
        }
        while left.engine.take_event().is_some() {}
        while right.engine.take_event().is_some() {}
    }
}

fn on(runner: &Runner) -> Option<Medium> {
    runner.neighbours().first().and_then(|near| runner.medium_of(near.active?))
}

#[test]
fn a_neighbour_on_two_links_gets_each_message_once() {
    let (mut left, mut right) = nodes();
    let radio = pair(&mut left, &mut right, Medium::EspNow);
    let bluetooth = pair(&mut left, &mut right, Medium::Bluetooth);
    run(&mut left, &mut right, 0, 3 * HELLO_MS);
    assert_eq!(left.neighbours().len(), 1, "one node on two links is one neighbour");
    assert_eq!(left.neighbours()[0].paths.len(), 2);

    let mut now = 3 * HELLO_MS;
    for _ in 0..5 {
        mine(&mut left.engine, now);
        run(&mut left, &mut right, now, now + 2 * HELLO_MS);
        now += 2 * HELLO_MS;
    }
    assert_eq!(right.engine.chain().height(), 5);
    assert!(!radio.heard.lock().unwrap().is_empty());
    let twice = bluetooth.heard.lock().unwrap();
    assert!(twice.is_empty(), "{} chain messages came over both links", twice.len());
}

#[test]
fn traffic_moves_at_once_and_comes_back_only_when_the_better_link_is_steady() {
    let (mut left, mut right) = nodes();
    let radio = pair(&mut left, &mut right, Medium::EspNow);
    let bluetooth = pair(&mut left, &mut right, Medium::Bluetooth);
    run(&mut left, &mut right, 0, 3 * HELLO_MS);
    assert_eq!(on(&left), Some(Medium::EspNow));

    *radio.cut.lock().unwrap() = true;
    let cut_at = 3 * HELLO_MS;
    run(&mut left, &mut right, cut_at, cut_at + SILENT_MS + HELLO_MS);
    assert_eq!(on(&left), Some(Medium::Bluetooth));
    assert_eq!(on(&right), Some(Medium::Bluetooth));

    let mut now = cut_at + SILENT_MS + HELLO_MS;
    mine(&mut left.engine, now);
    run(&mut left, &mut right, now, now + 2 * HELLO_MS);
    now += 2 * HELLO_MS;
    assert_eq!(right.engine.chain().height(), 1, "the block crosses on the link that is left");
    assert!(!bluetooth.heard.lock().unwrap().is_empty());

    *radio.cut.lock().unwrap() = false;
    run(&mut left, &mut right, now, now + STEADY_MS - 2 * HELLO_MS);
    assert_eq!(on(&left), Some(Medium::Bluetooth), "a link back for a moment wins nothing");
    now += STEADY_MS - 2 * HELLO_MS;
    run(&mut left, &mut right, now, now + 4 * HELLO_MS);
    assert_eq!(on(&left), Some(Medium::EspNow));
    let moves: u32 = left.neighbours().iter().map(|near| near.moves).sum();
    assert_eq!(moves, 2);
}

#[test]
fn half_an_hour_of_good_links_means_no_moves_and_no_dials_after_connecting() {
    let (mut left, mut right) = nodes();
    pair(&mut left, &mut right, Medium::EspNow);
    pair(&mut left, &mut right, Medium::Bluetooth);
    left.add_address(1, "aa:bb:cc:dd:ee:ff".into());
    right.add_address(1, "11:22:33:44:55:66".into());
    run(&mut left, &mut right, 0, 5 * HELLO_MS);
    let connecting = left.dials() + right.dials();
    assert!(connecting <= 2, "each end dials at most once to connect, not {connecting} times");
    let half_an_hour = 30 * 60 * 1_000;
    run(&mut left, &mut right, 5 * HELLO_MS, half_an_hour);
    assert_eq!(left.dials() + right.dials(), connecting, "a redial while connected");
    assert_eq!(left.neighbours()[0].moves, 0);
}

#[test]
fn a_better_rank_takes_effect_on_the_next_settle_and_is_the_only_one_used() {
    let (mut left, mut right) = nodes();
    let radio = pair(&mut left, &mut right, Medium::EspNow);
    let bluetooth = pair(&mut left, &mut right, Medium::Bluetooth);
    let rank = olc_node::Rank::parse("wired,bluetooth,espnow,wifi").unwrap();
    left.set_rank(rank);
    right.set_rank(rank);
    run(&mut left, &mut right, 0, 3 * HELLO_MS);
    mine(&mut left.engine, 3 * HELLO_MS);
    run(&mut left, &mut right, 3 * HELLO_MS, 6 * HELLO_MS);
    assert_eq!(right.engine.chain().height(), 1);
    assert!(radio.heard.lock().unwrap().is_empty());
    assert!(!bluetooth.heard.lock().unwrap().is_empty());
}
