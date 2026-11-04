//! The pool as the window reads it: the pool this app runs, the pools its miners joined, and
//! every miner that could work, each board with the one reason it cannot when it cannot.
use super::coins::format_coins;
use super::pool_plan::{Join, Method, Who};
use crate::fleet::{BoardRow, Connection};
use olc_node::Hello;
use olc_pool::{BlockState, ClientView, FoundTotal, Link};
use olc_wallet::format_address;
use std::net::{IpAddr, UdpSocket};

/// The first build whose blocks carry Bitcoin's header, the only work a pool hands out.
pub const FIRST_POOL_BUILD: u32 = 17;
/// The first build that passes pool work on to a board it hears, and takes work passed on to it.
pub const FIRST_LEND_BUILD: u32 = 19;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WorkerRow {
    /// As it signed in: `address.worker`.
    pub name: String,
    /// The part after the address.
    pub worker: String,
    pub address: String,
    pub speed: u64,
    /// Seconds since its last share.
    pub last_share_secs: Option<u64>,
    pub accepted: u64,
    pub refused: u64,
    /// Shares that came after a new block ended their job: work lost to timing, not a fault.
    pub stale: u64,
    pub paid: u64,
    pub difficulty: u64,
    pub online: bool,
    /// The miner here it is, if it is one.
    pub here: Option<Who>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FoundRow {
    pub height: u64,
    pub value: u64,
    pub state: BlockState,
    pub pool_fee: u64,
    pub secs_ago: u64,
    /// Ripe, but its payment waits for the locked wallet to open.
    pub held: bool,
    /// Where its coins went, once paid: worker and amount.
    pub parts: Vec<(String, u64)>,
}

/// A miner here working in a pool, this app's or another's.
#[derive(Clone, Debug, PartialEq)]
pub struct LinkRow {
    pub join: Join,
    pub label: String,
    pub view: ClientView,
    /// What went wrong handing it work, as a sentence's key.
    pub trouble: Option<&'static str>,
    /// It works in the pool this app runs.
    pub here: bool,
}

/// A miner that could work in a pool, and the reason it cannot when it cannot.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Choice {
    pub who: Option<Who>,
    pub label: String,
    /// The board it is heard through, for a board on the radio.
    pub through: Option<String>,
    /// The key of the one sentence saying why it cannot take pool work.
    pub why_not: Option<&'static str>,
    pub speed: u64,
    /// Mining for itself, as a board heard by radio last said.
    pub mining: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PoolView {
    pub running: bool,
    pub port: u16,
    pub open: bool,
    /// What others type to join: `host:port` for each address this computer has.
    pub reach: Vec<String>,
    pub method: Method,
    pub last_shares: u64,
    pub fee_bps: u32,
    pub fixed_difficulty: u64,
    /// The chain's difficulty the blocks are sealed at.
    pub block_difficulty: u64,
    pub workers: Vec<WorkerRow>,
    pub found: Vec<FoundRow>,
    /// Paid blocks older than those in `found`, so the totals count every block past the list.
    pub let_go: FoundTotal,
    pub links: Vec<LinkRow>,
    pub choices: Vec<Choice>,
    /// Boards heard through another that the pool switched off because they never took its work.
    pub switched_off: Vec<SwitchedOff>,
    /// A ripe block's payout waits for the wallet to open: it goes out then.
    pub waits_for_wallet: bool,
    /// Why the pool did not start, as a sentence's key and its detail.
    pub error: Option<(&'static str, String)>,
}

/// A board the pool switched off, kept until it is switched on again.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SwitchedOff {
    pub who: Who,
    /// The name it signed in to the pool with.
    pub user: String,
    /// The key of the sentence saying why and what to do.
    pub why: &'static str,
    /// The board on the cable it was heard through, for the sentence.
    pub relay: String,
    /// When, in Unix ms, so a page turns its switch off once.
    pub at_ms: u64,
}

impl PoolView {
    pub fn link_of(&self, who: Who) -> Option<&LinkRow> {
        self.links.iter().find(|row| row.join.who == who)
    }

    /// Coins the pool's blocks brought, and how many it found, those no longer listed included.
    pub fn found_total(&self) -> (u64, u64) {
        let kept = self.found.iter().filter(|row| row.state != BlockState::Lost);
        let start = (self.let_go.value, self.let_go.blocks);
        kept.fold(start, |(value, count), row| (value.saturating_add(row.value), count + 1))
    }
}

/// `iNN` as a number, for builds that say so.
fn build_number(build: &str) -> Option<u32> {
    build.trim().strip_prefix('i')?.split(|c: char| !c.is_ascii_digit()).next()?.parse().ok()
}

/// Why a board the fleet hears cannot take pool work, if it cannot.
pub fn board_reason(row: &BoardRow) -> Option<&'static str> {
    let seen = &row.seen;
    if !seen.online {
        return Some("pool.why_silent");
    }
    if seen.old_format {
        return Some("pool.why_old");
    }
    match build_number(&seen.beacon.build) {
        Some(build) if build < FIRST_POOL_BUILD => Some("pool.why_old"),
        _ => None,
    }
}

/// Every miner here that could work: the app first, then the miners on this computer, the boards
/// the fleet reaches, cable or radio, and the boards heard only as nodes, each with the link of
/// this app it was heard on.
/// A miner on this computer is given as its id, name and speed, with no speed when it is not running.
pub fn choices(locals: &[(u32, String, Option<u64>)], rows: &[BoardRow], hellos: &[(&Hello, Option<usize>)], own_speed: u64) -> Vec<Choice> {
    let mut all = vec![Choice { who: Some(Who::Own), speed: own_speed, mining: own_speed > 0, ..Choice::default() }];
    all.extend(locals.iter().map(|(id, name, speed)| Choice {
        who: Some(Who::Node(*id)),
        label: name.clone(),
        through: None,
        why_not: speed.is_none().then_some("pool.why_not_running"),
        speed: speed.unwrap_or_default(),
        mining: speed.is_some(),
    }));
    all.extend(rows.iter().map(|row| Choice {
        who: Some(Who::Board(row.seen.id)),
        label: row.seen.beacon.name.clone(),
        through: row.through.clone(),
        why_not: board_reason(row),
        speed: u64::from(row.seen.beacon.hashrate),
        mining: row.seen.beacon.mining,
    }));
    let reached: Vec<[u8; 20]> = rows.iter().map(|row| row.seen.beacon.address).collect();
    // A board heard only as a node, passed on by the board on one of this app's cables. That board
    // passes work on to it when both run a build that can.
    let heard_only = hellos.iter().filter(|(hello, _)| !reached.contains(&hello.address));
    all.extend(heard_only.map(|(hello, link)| {
        let label = if hello.card.name.is_empty() { short_address(&format_address(&hello.address)) } else { hello.card.name.clone() };
        let (speed, mining) = (hello.card.hashrate, hello.card.mining);
        match (relay_of(rows, *link), lends(&hello.card.build)) {
            (Some(relay), true) => {
                let through = Some(relay.seen.beacon.name.clone());
                Choice { who: Some(Who::Lent(hello.address)), label, through, why_not: None, speed, mining }
            }
            _ => {
                let medium = (!hello.card.via.0.is_empty()).then(|| hello.card.via.0.clone());
                let through = on_cable(rows, *link).map(|row| row.seen.beacon.name.clone()).or(medium);
                Choice { who: None, label, through, why_not: Some("pool.why_heard_only"), speed, mining }
            }
        }
    }));
    all
}

fn lends(build: &str) -> bool {
    build_number(build).is_some_and(|build| build >= FIRST_LEND_BUILD)
}

/// The board on this app's `link` that passes pool work on, when it runs a build that can.
pub fn relay_of(rows: &[BoardRow], link: Option<usize>) -> Option<&BoardRow> {
    let relay = on_cable(rows, link)?;
    (relay.seen.online && lends(&relay.seen.beacon.build)).then_some(relay)
}

/// The board on the cable that is this app's `link`.
fn on_cable(rows: &[BoardRow], link: Option<usize>) -> Option<&BoardRow> {
    let link = link?;
    rows.iter().find(|row| row.seen.connection == Connection::Direct { link })
}

pub fn short_address(address: &str) -> String {
    match address.len() > 14 {
        true => format!("{}…{}", &address[..8], &address[address.len() - 4..]),
        false => address.to_string(),
    }
}

pub fn link_trouble(view: &ClientView) -> Option<&'static str> {
    match view.link {
        Link::Unreachable => Some("pool.link_unreachable"),
        Link::NameRefused => Some("pool.link_name_refused"),
        Link::Foreign => Some("pool.link_foreign"),
        Link::Dialling | Link::Working => None,
    }
}

/// This computer's address on its network, as others reach it: the one a packet out would leave
/// from. Nothing is sent.
pub fn lan_address() -> Option<IpAddr> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("192.0.2.1:9").ok()?;
    let address = socket.local_addr().ok()?.ip();
    (!address.is_loopback() && !address.is_unspecified()).then_some(address)
}

/// What others type to join a pool on `port`: this computer's network address when the pool is
/// open, this computer alone when it is not. The network is looked up only for an open pool, and
/// not at all when a scene stands an address in.
pub fn reach(port: u16, open: bool, stand_in: Option<IpAddr>) -> Vec<String> {
    let local = format!("127.0.0.1:{port}");
    let network = if open { stand_in.or_else(lan_address) } else { None };
    match network {
        Some(address) => vec![format!("{address}:{port}"), local],
        None => vec![local],
    }
}

fn link_word(link: Link) -> &'static str {
    match link {
        Link::Dialling => "connecting",
        Link::Working => "working",
        Link::NameRefused => "its name was refused",
        Link::Unreachable => "cannot reach the pool",
        Link::Foreign => "the pool hands out work for another coin",
    }
}

fn found_word(row: &FoundRow) -> &'static str {
    match row.state {
        BlockState::Waiting if row.held => "spendable, waiting for the wallet to open",
        BlockState::Sent => "sent",
        BlockState::Waiting => "waiting to mature",
        BlockState::Paid => "paid out",
        BlockState::Lost => "lost to another block",
    }
}

/// How the pools stand, in lines for the command line.
pub fn listing(view: &PoolView) -> String {
    let mut lines = Vec::new();
    if view.running {
        let method = match view.method {
            Method::Proportional => "proportional".to_string(),
            Method::LastShares => format!("PPLNS over the last {} shares", view.last_shares),
        };
        let fee = format!("{}.{:02} %", view.fee_bps / 100, view.fee_bps % 100);
        lines.push(format!("Pool on port {}, {method}, fee {fee}. Miners join at {}.", view.port, view.reach.join(" or ")));
        for row in &view.workers {
            let last = row.last_share_secs.map_or("no share yet".to_string(), |secs| format!("last share {secs} s ago"));
            lines.push(format!(
                "  {}: {} H/s, {} taken, {} refused, paid {}, {last}.",
                row.name,
                row.speed,
                row.accepted,
                row.refused,
                format_coins(row.paid)
            ));
        }
        for row in &view.found {
            lines.push(format!("  Block {}: {}, {}.", row.height, format_coins(row.value), found_word(row)));
        }
    } else {
        lines.push("The pool is not running.".into());
    }
    if view.waits_for_wallet {
        lines.push("The wallet is locked, so payouts wait. They go out when it opens: olc wallet unlock SECONDS.".into());
    }
    for row in &view.links {
        let name = if row.label.is_empty() { "This computer" } else { row.label.as_str() };
        let (taken, refused) = (row.view.accepted, row.view.refused);
        let link = link_word(row.view.link);
        lines.push(format!("{name} mines in {} as {}: {link}, {taken} taken, {refused} refused.", row.join.address, row.join.user));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_build_says_whether_it_can_take_pool_work() {
        assert_eq!(build_number("i17"), Some(17));
        assert_eq!(build_number("i9-dirty"), Some(9));
        assert_eq!(build_number("dev"), None);
    }

    fn board_zero(build: &str) -> Hello {
        use olc_core::coin::CoinConfig;
        use olc_node::hello::{Card, Via};
        let card = Card { name: "보드제로".into(), build: build.into(), hashrate: 9_000, via: Via("espnow".into()), ..Card::default() };
        Hello { coin: CoinConfig::olc(), tip: [0; 32], height: 3, work: 0, address: [7; 20], card }
    }

    /// Board one, on this app's cable on link 2.
    fn board_one(build: &str) -> BoardRow {
        use crate::fleet::Seen;
        let beacon = olc_control::Beacon { name: "보드원".into(), address: [1; 20], build: build.into(), ..Default::default() };
        let seen = Seen { id: [1; 6], beacon, connection: Connection::Direct { link: 2 }, online: true, silent_for: std::time::Duration::ZERO, old_format: false };
        BoardRow { seen, on_cable: true, through: None, job: None }
    }

    /// Board zero has no cable and no control link: the app hears it only as a node, passed on by
    /// board one over the radio. On an old build it is still listed, with what it is heard through
    /// and the one line saying why it cannot take pool work, as a stopped miner here is.
    #[test]
    fn a_board_heard_only_through_another_is_listed_with_why_it_cannot_work() {
        let zero = board_zero("i17");
        let locals = [(1, "worker-1".to_string(), Some(500)), (2, "worker-2".to_string(), None)];
        let all = choices(&locals, &[], &[(&zero, Some(2))], 1_000);
        let names: Vec<&str> = all.iter().map(|choice| choice.label.as_str()).collect();
        assert_eq!(names, ["", "worker-1", "worker-2", "보드제로"], "this computer, both miners and board zero");
        assert_eq!(all[0].who, Some(Who::Own));
        assert_eq!(all[2].why_not, Some("pool.why_not_running"));
        let board = &all[3];
        assert_eq!(board.who, None, "nothing here can hand it work");
        assert_eq!(board.why_not, Some("pool.why_heard_only"));
        assert_eq!(board.through.as_deref(), Some("espnow"));
        assert_eq!(board.speed, 9_000);
    }

    /// With the new build on board zero and on board one, the board on the cable it was heard
    /// through, board zero is a worker reached through board one.
    #[test]
    fn a_board_heard_through_a_board_that_passes_work_on_is_a_worker() {
        let new = format!("i{FIRST_LEND_BUILD}");
        let zero = board_zero(&new);
        let all = choices(&[], &[board_one(&new)], &[(&zero, Some(2))], 1_000);
        let board = all.iter().find(|choice| choice.label == "보드제로").unwrap();
        assert_eq!(board.who, Some(Who::Lent([7; 20])));
        assert_eq!(board.why_not, None);
        assert_eq!(board.through.as_deref(), Some("보드원"));
        // Either board on an older build, or a hello from a link no board of this app is on, keeps
        // the reason, and names the board it is heard through when that board is on a cable here.
        let old = format!("i{}", FIRST_LEND_BUILD - 1);
        let cases = [(old.as_str(), new.as_str(), Some(2), "보드원"), (new.as_str(), old.as_str(), Some(2), "보드원"), (new.as_str(), new.as_str(), Some(5), "espnow")];
        for (zero_build, one_build, link, through) in cases {
            let zero = board_zero(zero_build);
            let all = choices(&[], &[board_one(one_build)], &[(&zero, link)], 1_000);
            let board = all.iter().find(|choice| choice.label == "보드제로").unwrap();
            assert_eq!((board.who, board.why_not), (None, Some("pool.why_heard_only")), "{zero_build} {one_build} {link:?}");
            assert_eq!(board.through.as_deref(), Some(through));
        }
    }
}
