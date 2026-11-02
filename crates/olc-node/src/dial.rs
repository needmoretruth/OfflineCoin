//! Reaching a node over links that must connect first, in the way of Happy Eyeballs (RFC 8305):
//! ways are tried a moment apart and overlap, the first to work cancels the rest, and a way that
//! fails waits twice as long each time. Nothing is dialled while a neighbour has a working link
//! as good as what could be dialled.
use crate::neighbours::{Neighbours, CLOSE_MS};
use alloc::string::String;
use alloc::vec::Vec;

/// Gap between two ways of reaching the same node.
pub const STAGGER_MS: u64 = 250;
const FIRST_WAIT_MS: u64 = 1_000;
const LONGEST_WAIT_MS: u64 = 60_000;
/// A dial with no answer by now has failed, in case the link never said so.
const GIVE_UP_MS: u64 = 10_000;
/// A sign of life on a better link is good for this long.
const FRESH_MS: u64 = 5_000;
/// Two nodes that want each other should not both dial. The higher id waits this long.
const HIGHER_WAITS_MS: u64 = 1_500;
const MOST: usize = 16;

/// One way to reach a node.
#[derive(Clone, Debug)]
struct Place {
    link: usize,
    address: String,
    /// Who is there, once known.
    node: Option<u64>,
    /// A person gave this address, so it stays when nothing is heard from it.
    chosen: bool,
    /// When the node last showed itself there without a connection.
    seen_ms: Option<u64>,
    /// Its number on the link while connected.
    number: Option<u32>,
    opened_ms: u64,
    dialling_since: Option<u64>,
    wanted_since: Option<u64>,
    next_ms: u64,
    wait_ms: u64,
}

impl Place {
    fn new(link: usize, address: String, node: Option<u64>, chosen: bool) -> Self {
        let wait_ms = FIRST_WAIT_MS;
        Self { link, address, node, chosen, seen_ms: None, number: None, opened_ms: 0, dialling_since: None, wanted_since: None, next_ms: 0, wait_ms }
    }

    fn failed(&mut self, now: u64) {
        self.dialling_since = None;
        self.next_ms = now + self.wait_ms;
        self.wait_ms = (self.wait_ms * 2).min(LONGEST_WAIT_MS);
    }
}

/// What the links must do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Order {
    Dial { link: usize, address: String },
    Cancel { link: usize, address: String },
    /// A connection the far end stopped speaking on, which nothing else would end.
    Close { link: usize, number: u32 },
}

#[derive(Default)]
pub struct Dialer {
    places: Vec<Place>,
    /// When the last dial of each node started, so ways stay a moment apart.
    started: Vec<(u64, u64)>,
    /// Every dial ever started. Stays still while everything is connected.
    pub dials: u64,
}

impl Dialer {
    fn find(&mut self, link: usize, address: &str) -> Option<&mut Place> {
        self.places.iter_mut().find(|place| place.link == link && place.address == address)
    }

    fn room(&mut self) {
        if self.places.len() >= MOST {
            if let Some(at) = self.places.iter().position(|place| !place.chosen && place.number.is_none()) {
                self.places.remove(at);
            }
        }
    }

    /// An address a person gave. It is dialled until it answers, and again if it stops and
    /// nothing else reaches that node.
    pub fn add(&mut self, link: usize, address: String) {
        if self.find(link, &address).is_none() {
            self.room();
            self.places.push(Place::new(link, address, None, true));
        }
    }

    /// Forgets every address. Connections that are open stay as they are.
    pub fn clear(&mut self) {
        self.places.clear();
    }

    /// A node showed itself on a link that must connect before it can carry anything.
    pub fn seen(&mut self, link: usize, address: String, node: u64, now: u64) {
        if self.find(link, &address).is_none() {
            self.room();
            self.places.push(Place::new(link, address.clone(), Some(node), false));
        }
        if let Some(place) = self.find(link, &address) {
            place.node = Some(node);
            place.seen_ms = Some(now);
        }
    }

    /// A dial worked. Everything else tried for the same node stops.
    pub fn opened(&mut self, link: usize, address: &str, number: u32, now: u64) -> Vec<Order> {
        let Some(place) = self.find(link, address) else {
            return Vec::new();
        };
        place.number = Some(number);
        place.opened_ms = now;
        place.dialling_since = None;
        place.wanted_since = None;
        place.wait_ms = FIRST_WAIT_MS;
        place.next_ms = 0;
        let node = place.node;
        let same = |one: &&mut Place| node.is_some() && one.node == node && one.dialling_since.is_some();
        let rest = self.places.iter_mut().filter(same);
        let cancel = |one: &mut Place| {
            one.dialling_since = None;
            Order::Cancel { link: one.link, address: one.address.clone() }
        };
        rest.map(cancel).collect()
    }

    pub fn failed(&mut self, link: usize, address: &str, now: u64) {
        if let Some(place) = self.find(link, address) {
            place.failed(now);
        }
    }

    pub fn closed(&mut self, link: usize, number: u32) {
        let held = |place: &&mut Place| place.link == link && place.number == Some(number);
        if let Some(place) = self.places.iter_mut().find(held) {
            place.number = None;
        }
    }

    /// A Hello said who is at the far end of a connection this node dialled.
    pub fn learn(&mut self, link: usize, number: u32, node: u64) {
        let held = |place: &&mut Place| place.link == link && place.number == Some(number);
        if let Some(place) = self.places.iter_mut().find(held) {
            place.node = Some(node);
        }
    }

    /// Whether connecting here would help, leaving aside who goes first.
    fn useful(place: &Place, table: &Neighbours, now: u64) -> bool {
        let Some(node) = place.node else {
            return true;
        };
        if table.is_heard_on(node, place.link) {
            return false;
        }
        let fresh = place.seen_ms.is_some_and(|seen| now.saturating_sub(seen) < FRESH_MS);
        match table.active_of(node) {
            None => fresh || place.chosen,
            Some(active) => fresh && table.beats(place.link, active),
        }
    }

    /// What to do now, given who has a working link.
    pub fn step(&mut self, table: &Neighbours, me: u64, now: u64) -> Vec<Order> {
        let mut orders = Vec::new();
        for at in 0..self.places.len() {
            let mut place = self.places[at].clone();
            if let (Some(number), Some(node)) = (place.number, place.node) {
                let settled = now.saturating_sub(place.opened_ms) >= CLOSE_MS;
                if !settled || !table.is_gone_from(node, place.link, now) {
                    continue;
                }
                orders.push(Order::Close { link: place.link, number });
                self.places[at].number = None;
                continue;
            }
            if place.number.is_some() {
                continue;
            }
            if place.dialling_since.is_some_and(|since| now.saturating_sub(since) >= GIVE_UP_MS) {
                orders.push(Order::Cancel { link: place.link, address: place.address.clone() });
                place.failed(now);
            }
            let useful = Self::useful(&place, table, now);
            place.wanted_since = place.wanted_since.or(Some(now)).filter(|_| useful);
            let my_turn = match (place.node, place.wanted_since) {
                (Some(node), Some(since)) if node < me => now.saturating_sub(since) >= HIGHER_WAITS_MS,
                _ => true,
            };
            if useful && my_turn && place.dialling_since.is_none() && now >= place.next_ms {
                let group = place.node.unwrap_or(at as u64 | 1 << 63);
                let last = self.started.iter().find(|(node, _)| *node == group).map(|(_, ms)| *ms);
                if last.is_none_or(|ms| now.saturating_sub(ms) >= STAGGER_MS) {
                    self.started.retain(|(node, _)| *node != group);
                    if self.started.len() >= MOST {
                        self.started.remove(0);
                    }
                    self.started.push((group, now));
                    place.dialling_since = Some(now);
                    self.dials += 1;
                    orders.push(Order::Dial { link: place.link, address: place.address.clone() });
                }
            }
            self.places[at] = place;
        }
        // A neighbour heard again owes nothing to its past failures, unless this way is one it
        // still wants: that is the failing dial the wait is there to slow down.
        for at in 0..self.places.len() {
            let heard = self.places[at].node.is_some_and(|node| table.active_of(node).is_some());
            if heard && !Self::useful(&self.places[at], table, now) {
                self.places[at].wait_ms = FIRST_WAIT_MS;
                self.places[at].next_ms = 0;
            }
        }
        orders
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::medium::Medium;
    use crate::neighbours::SILENT_MS;

    const RADIO: usize = 0;
    const BLUETOOTH: usize = 1;
    const WIFI: usize = 2;
    const ME: u64 = 5;
    const THEM: u64 = 9;

    fn table() -> Neighbours {
        let mut table = Neighbours::default();
        for medium in [Medium::EspNow, Medium::Bluetooth, Medium::Wifi] {
            table.add_link(medium);
        }
        table
    }

    fn dials(orders: &[Order]) -> usize {
        orders.iter().filter(|order| matches!(order, Order::Dial { .. })).count()
    }

    #[test]
    fn nothing_is_dialled_while_a_good_link_works() {
        let (mut table, mut dialer) = (table(), Dialer::default());
        table.name(RADIO, 1, THEM, 0);
        table.settle(0);
        dialer.seen(BLUETOOTH, "ble".into(), THEM, 0);
        for now in (0..3_000).step_by(100) {
            table.name(RADIO, 1, THEM, now);
            dialer.seen(BLUETOOTH, "ble".into(), THEM, now);
            assert!(dialer.step(&table, ME, now).is_empty());
        }
        assert_eq!(dialer.dials, 0);
    }

    #[test]
    fn a_better_link_that_shows_itself_is_dialled_once() {
        let (mut table, mut dialer) = (table(), Dialer::default());
        table.name(BLUETOOTH, 1, THEM, 0);
        table.settle(0);
        dialer.seen(WIFI, "host:7000".into(), THEM, 0);
        assert_eq!(dials(&dialer.step(&table, ME, 0)), 1);
        dialer.opened(WIFI, "host:7000", 3, 100);
        table.name(WIFI, 3, THEM, 100);
        table.name(BLUETOOTH, 1, THEM, 100);
        assert_eq!(dials(&dialer.step(&table, ME, 200)), 0);
        assert_eq!(dialer.dials, 1);
    }

    #[test]
    fn a_node_with_no_link_gets_every_way_a_moment_apart_and_the_first_cancels_the_rest() {
        let (table, mut dialer) = (table(), Dialer::default());
        dialer.seen(BLUETOOTH, "ble".into(), THEM, 0);
        dialer.seen(WIFI, "host:7000".into(), THEM, 0);
        assert_eq!(dials(&dialer.step(&table, ME, 0)), 1);
        assert_eq!(dials(&dialer.step(&table, ME, STAGGER_MS - 1)), 0);
        assert_eq!(dials(&dialer.step(&table, ME, STAGGER_MS)), 1);
        let cancelled = dialer.opened(WIFI, "host:7000", 1, STAGGER_MS);
        assert_eq!(cancelled, [Order::Cancel { link: BLUETOOTH, address: "ble".into() }]);
    }

    #[test]
    fn a_place_nobody_answers_waits_twice_as_long_each_time() {
        let (table, mut dialer) = (table(), Dialer::default());
        dialer.add(WIFI, "host:7000".into());
        let mut times = Vec::new();
        for now in (0..200_000).step_by(100) {
            if dials(&dialer.step(&table, ME, now)) == 1 {
                times.push(now);
                dialer.failed(WIFI, "host:7000", now);
            }
        }
        assert_eq!(times[..4], [0, 1_000, 3_000, 7_000]);
        assert!(times.windows(2).all(|pair| pair[1] - pair[0] <= LONGEST_WAIT_MS));
    }

    #[test]
    fn a_dial_that_never_answers_counts_as_failed() {
        let (table, mut dialer) = (table(), Dialer::default());
        dialer.add(WIFI, "host:7000".into());
        dialer.step(&table, ME, 0);
        let orders = dialer.step(&table, ME, GIVE_UP_MS);
        assert_eq!(orders, [Order::Cancel { link: WIFI, address: "host:7000".into() }]);
    }

    #[test]
    fn a_connected_place_is_left_alone_until_it_closes() {
        let (mut table, mut dialer) = (table(), Dialer::default());
        dialer.add(WIFI, "host:7000".into());
        dialer.step(&table, ME, 0);
        dialer.opened(WIFI, "host:7000", 4, 0);
        table.name(WIFI, 4, THEM, 10);
        dialer.learn(WIFI, 4, THEM);
        table.settle(10);
        for now in (0..600_000).step_by(1_000) {
            table.name(WIFI, 4, THEM, now);
            table.settle(now);
            assert_eq!(dials(&dialer.step(&table, ME, now)), 0);
        }
        dialer.closed(WIFI, 4);
        table.lost(WIFI, 4);
        table.settle(600_000);
        assert_eq!(dials(&dialer.step(&table, ME, 600_000)), 1);
    }

    #[test]
    fn a_connection_gone_silent_is_closed_and_dialled_again() {
        let (mut table, mut dialer) = (table(), Dialer::default());
        dialer.add(WIFI, "host:7000".into());
        dialer.step(&table, ME, 0);
        dialer.opened(WIFI, "host:7000", 4, 0);
        table.name(WIFI, 4, THEM, 0);
        dialer.learn(WIFI, 4, THEM);
        table.settle(0);
        table.settle(SILENT_MS);
        assert_eq!(dialer.step(&table, ME, SILENT_MS), [], "busy is not gone");
        table.settle(CLOSE_MS);
        let orders = dialer.step(&table, ME, CLOSE_MS);
        assert_eq!(orders, [Order::Close { link: WIFI, number: 4 }]);
        assert_eq!(dials(&dialer.step(&table, ME, CLOSE_MS + 1)), 1);
    }

    #[test]
    fn of_two_nodes_that_want_each_other_the_lower_id_goes_first() {
        let (table, mut higher) = (table(), Dialer::default());
        higher.seen(BLUETOOTH, "ble".into(), ME, 0);
        assert_eq!(dials(&higher.step(&table, THEM, 0)), 0);
        higher.seen(BLUETOOTH, "ble".into(), ME, HIGHER_WAITS_MS);
        assert_eq!(dials(&higher.step(&table, THEM, HIGHER_WAITS_MS)), 1);
    }

    #[test]
    fn a_better_way_that_keeps_failing_waits_longer_each_time() {
        let mut table = table();
        table.name(BLUETOOTH, 1, THEM, 0);
        table.settle(0);
        let mut dialer = Dialer::default();
        let mut started = Vec::new();
        for now in (0..30_000).step_by(100) {
            table.name(BLUETOOTH, 1, THEM, now);
            table.settle(now);
            dialer.seen(WIFI, "here".into(), THEM, now);
            if dials(&dialer.step(&table, ME, now)) > 0 {
                started.push(now);
                dialer.failed(WIFI, "here", now + 50);
            }
        }
        assert!((2..=5).contains(&started.len()), "{started:?}");
    }
}
