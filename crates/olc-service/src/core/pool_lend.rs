//! Pool work for a board heard only through the board on one of this app's cables. The fleet signs
//! the work and sends it to that board, which passes it on; the shares come back to the fleet.
//! See `olc_control::lend`.
use super::pool_run::PoolCx;
use super::pool_view::{relay_of, short_address};
use olc_control::BoardId;
use olc_core::BlockHeader;
use olc_wallet::format_address;
use std::time::{Duration, Instant};

/// How long a board heard through another has, from the first work sent, to say it took it.
/// Both boards answer the moment the work reaches them, well within a second; this leaves room
/// for three sends over a radio that drops a packet now and then, and the reason still shows
/// while the person who switched the board on is looking at the page.
pub const ANSWER_WITHIN: Duration = Duration::from_secs(15);
/// Work nobody has said they took goes out again this often.
const SEND_AGAIN: Duration = Duration::from_secs(5);

/// Lent work from its first send until the board says it took it.
#[derive(Clone, Copy, Debug)]
pub struct Watch {
    since: Instant,
    last: Instant,
    taken: bool,
}

/// What the boards have said about lent work so far.
#[derive(Debug, PartialEq, Eq)]
pub enum Said {
    Taken,
    Waiting,
    /// Time to send the work again.
    Again,
    /// Silent too long: the key of the sentence saying which board and what to do.
    Silent(&'static str),
}

impl Watch {
    pub fn start() -> Self {
        let now = Instant::now();
        Self { since: now, last: now, taken: false }
    }

    pub fn sent(&mut self) {
        self.last = Instant::now();
    }

    /// The board on the cable saying it passed the work on, and the worker saying it took it,
    /// each counted only since this work first went out.
    pub fn check(&mut self, worker: &[u8; 20], cx: &PoolCx) -> Said {
        if self.taken {
            return Said::Taken;
        }
        let heard = cx.fleet.map(|fleet| fleet.lent_heard(worker)).unwrap_or_default();
        let since = |at: Option<Instant>| at.is_some_and(|at| at >= self.since);
        if since(heard.taken) {
            self.taken = true;
            return Said::Taken;
        }
        if self.since.elapsed() >= ANSWER_WITHIN {
            return Said::Silent(match since(heard.passed) {
                true => "pool.lent_untrusted",
                false => "pool.relay_untrusted",
            });
        }
        match self.last.elapsed() >= SEND_AGAIN {
            true => Said::Again,
            false => Said::Waiting,
        }
    }
}

/// The board on the cable that passes work on to `worker`, by its name.
pub fn relay_name(worker: &[u8; 20], cx: &PoolCx) -> String {
    let rows = cx.fleet.map(|fleet| fleet.board_rows()).unwrap_or_default();
    let relay = relay_for(worker, cx).and_then(|id| rows.into_iter().find(|row| row.seen.id == id));
    relay.map(|row| row.seen.beacon.name).unwrap_or_default()
}

/// The board on this app's cable that passes work on to the board that mines to `worker`.
pub fn relay_for(worker: &[u8; 20], cx: &PoolCx) -> Option<BoardId> {
    let fleet = cx.fleet?;
    let (_, link) = cx.hellos.iter().find(|(hello, _)| hello.address == *worker)?;
    relay_of(&fleet.board_rows(), *link).map(|row| row.seen.id)
}

/// What the board calls itself, or its address when it gives no name.
pub fn lent_name(worker: &[u8; 20], hellos: &[(&olc_node::Hello, Option<usize>)]) -> String {
    let hello = hellos.iter().find(|(hello, _)| hello.address == *worker);
    let name = hello.map(|(hello, _)| hello.card.name.clone()).unwrap_or_default();
    match name.is_empty() {
        true => short_address(&format_address(worker)),
        false => name,
    }
}

/// Hands the board work, or stops it with `None`. Says whether the work went out.
pub fn lend(worker: [u8; 20], work: Option<(&BlockHeader, u64)>, cx: &PoolCx) -> bool {
    let (Some(fleet), Some(relay)) = (cx.fleet, relay_for(&worker, cx)) else {
        return false;
    };
    fleet.lend(relay, worker, work.map(|(header, difficulty)| (header.encode().to_vec(), difficulty)))
}

/// The shares the board sent back since the last call.
pub fn take_lent(worker: &[u8; 20], cx: &PoolCx) -> Vec<BlockHeader> {
    let Some(fleet) = cx.fleet else {
        return Vec::new();
    };
    let shares = fleet.take_lent_shares(worker);
    shares.iter().filter_map(|bytes| BlockHeader::decode(bytes)).collect()
}
