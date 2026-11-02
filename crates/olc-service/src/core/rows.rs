//! This app's own node and the boards as rows of the Network page.
use super::snapshot::NodeView;
use super::found::link_of;
use crate::discovery::{Entry, Identity, Reach, Work};
use crate::fleet::BoardRow;
use crate::local::{Holding, LocalView};
use olc_core::{CoinConfig, Hash};
use super::returning::is_returning;
use olc_platform::platform;
use olc_node::{Hello, Keeps, Medium};

pub const BOARD_KIND: &str = "esp32-s3";

pub fn hostname() -> String {
    platform().host_name()
}

/// What this program runs on, in the words a node gives for itself.
pub fn device_kind() -> String {
    match platform().device_model().contains("Raspberry") {
        true => "raspberry-pi".into(),
        false => "computer".into(),
    }
}

pub fn own_entry(view: &NodeView, identity: Option<Identity>) -> Entry {
    Entry {
        reach: Reach::ThisApp,
        address: view.port.map(|port| format!("127.0.0.1:{port}")),
        via: None,
        device: device_kind(),
        name: hostname(),
        identity,
        tip: None,
        height: Some(view.height),
        work_done: Some(view.work),
        build: env!("CARGO_PKG_VERSION").into(),
        work: match view.mining {
            true => Work::Mining(view.hashrate),
            false => Work::Idle,
        },
        keeps: view.keeps,
        age_secs: None,
    }
}

/// Nodes this app runs read live, rather than as they stood at the last look around, and appear
/// without waiting for one. Another program's node on this computer stays as it answered.
pub fn merge_local(entries: &mut Vec<Entry>, nodes: &[LocalView], coin: Option<&Identity>) {
    let Some(coin) = coin else {
        return;
    };
    for node in nodes {
        let Some(status) = &node.status else {
            continue;
        };
        // A node with no port still runs and mines, so it keeps its row.
        let address = status.port.map(|port| format!("127.0.0.1:{port}"));
        let same_node = |entry: &&mut Entry| {
            let genesis = entry.identity.as_ref().map(|found| found.genesis);
            let same_port = address.is_some() && entry.address == address;
            entry.reach == Reach::ThisComputer && same_port && genesis == Some(coin.genesis)
        };
        let live = Entry {
            reach: Reach::ThisComputer,
            address: address.clone(),
            via: None,
            device: device_kind(),
            name: node.spec.name.clone(),
            identity: Some(coin.clone()),
            tip: hash_of(&status.tip),
            height: Some(status.height),
            work_done: status.work.parse().ok(),
            build: env!("CARGO_PKG_VERSION").into(),
            work: match status.mining {
                true => Work::Mining(status.hashrate),
                false => Work::Idle,
            },
            keeps: match node.spec.holding {
                Holding::Everything => Keeps::Everything,
                Holding::Recent => Keeps::Recent,
                Holding::Headers => Keeps::Headers,
            },
            age_secs: None,
        };
        match entries.iter_mut().find(same_node) {
            Some(entry) => *entry = live,
            None => entries.push(live),
        }
    }
}

fn hash_of(text: &str) -> Option<Hash> {
    let mut hash = Hash::default();
    if text.len() != hash.len() * 2 || !text.is_ascii() {
        return None;
    }
    for (index, byte) in hash.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16).ok()?;
    }
    Some(hash)
}

pub fn reach_of_board(row: &BoardRow, link: Option<Medium>) -> Reach {
    match (row.on_cable, &row.through, link) {
        (true, ..) => Reach::Usb,
        (false, through, Some(medium)) => Reach::Link(medium, through.clone()),
        (false, through, None) => Reach::Radio(through.clone()),
    }
}

/// The coin a board's signed beacon names. An answer counts only if it agrees with the beacon,
/// and a coin known here needs no answer at all. Any other coin stays unknown until one comes.
/// `rules` is what the board said it runs by: one genesis can carry a fork and the coin it left.
pub fn coin_of_board(
    (genesis, rules): (Hash, Option<&str>),
    said: Option<Identity>,
    known: &[Identity],
) -> Option<Identity> {
    let fits = |coin: &Identity| {
        coin.genesis == genesis && rules.is_none_or(|id| coin.coin.rules_id() == id)
    };
    said.filter(fits).or_else(|| known.iter().find(|coin| fits(coin)).cloned())
}

/// True when a board on the coin this app runs goes by other rules, so it will part from this
/// node at the height they differ. A board that has not said stays the same coin.
pub fn on_other_rules(board: (Hash, Option<&str>), own: Option<&CoinConfig>) -> bool {
    let (genesis, rules) = board;
    let same_coin = own.is_some_and(|coin| coin.genesis().id() == genesis);
    same_coin && own.zip(rules).is_some_and(|(coin, id)| coin.rules_id() != id)
}

/// The rules this coin ran by before each of its forks, which a board may still be on.
pub fn earlier_rules(coin: &CoinConfig) -> Vec<Identity> {
    let earlier = std::iter::successors(coin.before_last_fork(), CoinConfig::before_last_fork);
    earlier.map(Identity::of).collect()
}

/// What a board holds, by its own hello.
pub fn keeps_of(said: Option<&Hello>) -> Keeps {
    said.map_or(Keeps::Unsaid, |hello| hello.card.keeps)
}

pub fn board_entry(row: &BoardRow, coin: Option<Identity>, said: Option<&Hello>) -> Entry {
    let beacon = &row.seen.beacon;
    Entry {
        reach: reach_of_board(row, said.and_then(link_of)),
        address: None,
        via: None,
        device: BOARD_KIND.into(),
        name: beacon.name.clone(),
        identity: coin,
        tip: Some(beacon.tip),
        height: Some(beacon.height),
        work_done: Some(beacon.work),
        build: beacon.build.clone(),
        work: match (row.seen.online, beacon.offline, beacon.mining) {
            (false, _, _) if is_returning(row) => Work::Restarting,
            (false, _, _) => Work::Silent,
            (true, true, _) => Work::Offline,
            (true, false, true) => Work::Mining(u64::from(beacon.hashrate)),
            (true, false, false) => Work::Idle,
        },
        keeps: keeps_of(said),
        age_secs: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local::{ChildStatus, Enforcement, Preset};

    fn found(coin: &CoinConfig) -> Entry {
        Entry {
            reach: Reach::ThisComputer,
            address: Some("127.0.0.1:7100".into()),
            via: None,
            device: "computer".into(),
            name: "node-0".into(),
            identity: Some(Identity::of(coin.clone())),
            tip: None,
            height: Some(3),
            work_done: Some(90),
            build: String::new(),
            work: Work::Idle,
            keeps: Keeps::Unsaid,
            age_secs: Some(20),
        }
    }

    fn running() -> Vec<LocalView> {
        vec![LocalView {
            spec: Preset::Esp32S3.spec(0),
            pid: Some(1),
            status: Some(ChildStatus {
                height: 40,
                work: "900".into(),
                port: Some(7100),
                ..ChildStatus::default()
            }),
            measured: None,
            memory_limit: 0,
            enforcement: Enforcement::Nothing,
            ended: None,
        }]
    }

    #[test]
    fn a_board_shows_the_coin_its_beacon_names() {
        let ours = Identity::of(CoinConfig::olc());
        let mut other = CoinConfig::olc();
        other.message = "Another genesis".into();
        let other = Identity::of(other);
        let known = [ours.clone()];

        let silent = coin_of_board((ours.genesis, None), None, &known);
        assert_eq!(silent.map(|coin| coin.genesis), Some(ours.genesis));
        assert!(coin_of_board((other.genesis, None), None, &known).is_none());

        let answered = coin_of_board((other.genesis, None), Some(other.clone()), &known);
        assert_eq!(answered.map(|coin| coin.genesis), Some(other.genesis));
        let lied = coin_of_board((ours.genesis, None), Some(other), &[]);
        assert!(lied.is_none());
    }

    fn forked() -> CoinConfig {
        let mut coin = CoinConfig::olc();
        let mut rules = coin.rules.clone();
        rules.min_fee += 1;
        coin.forks.push(olc_core::coin::Fork { from_height: 9_000, rules });
        coin
    }

    /// One genesis carries both sides of a fork, so the genesis alone called them the same coin.
    #[test]
    fn a_board_on_old_rules_is_not_the_fork_it_shares_a_genesis_with() {
        let (base, fork) = (CoinConfig::olc(), forked());
        let genesis = base.genesis().id();
        assert_eq!(genesis, fork.genesis().id());
        assert_ne!(base.rules_id(), fork.rules_id());

        let board = (genesis, Some(base.rules_id()));
        let board = (board.0, board.1.as_deref());
        assert!(on_other_rules(board, Some(&fork)));
        assert!(!on_other_rules(board, Some(&base)));
        assert!(!on_other_rules((genesis, None), Some(&fork)), "a board that has not said");

        let other = CoinConfig { message: "Another genesis".into(), ..CoinConfig::olc() };
        assert!(!on_other_rules((other.genesis().id(), Some("00000000")), Some(&base)));

        let known = [Identity::of(fork.clone()), Identity::of(base.clone())];
        let seen = coin_of_board(board, None, &known).expect("the coin it left is known");
        assert_eq!(seen.coin, base);
        let on_the_fork = fork.rules_id();
        let seen = coin_of_board((genesis, Some(on_the_fork.as_str())), None, &known);
        assert_eq!(seen.map(|found| found.coin), Some(fork.clone()));
        let unheard = coin_of_board((genesis, Some("00000000")), None, &known);
        assert!(unheard.is_none(), "rules nobody here runs must not read as one of these");
        assert_eq!(earlier_rules(&fork), [Identity::of(base)]);
    }

    #[test]
    fn a_name_is_not_a_rule() {
        let base = CoinConfig::olc();
        let renamed = CoinConfig { name: "Other words".into(), ticker: "OTH".into(), ..base.clone() };
        assert_eq!(base.rules_id(), renamed.rules_id());
        let mut cheaper = base.clone();
        cheaper.rules.min_fee = 1;
        assert_ne!(base.rules_id(), cheaper.rules_id());
    }

    #[test]
    fn a_running_node_reads_live_only_on_this_apps_coin() {
        let ours = CoinConfig::olc();
        let mut other = CoinConfig::olc();
        other.message = "Another genesis".into();
        let identity = Identity::of(ours.clone());

        let mut entries = vec![found(&ours)];
        merge_local(&mut entries, &running(), Some(&identity));
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].height, Some(40));
        assert_eq!(entries[0].age_secs, None);

        let mut entries = vec![found(&other)];
        merge_local(&mut entries, &running(), Some(&identity));
        assert_eq!(entries[0].height, Some(3));
        assert_eq!(entries[1].height, Some(40));
    }

    #[test]
    fn a_running_node_with_no_port_keeps_its_row() {
        let identity = Identity::of(CoinConfig::olc());
        let mut nodes = running();
        if let Some(status) = nodes[0].status.as_mut() {
            status.port = None;
        }
        let mut entries = Vec::new();
        merge_local(&mut entries, &nodes, Some(&identity));
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].address, None);
        assert_eq!(entries[0].height, Some(40));
    }

    #[test]
    fn a_running_node_needs_no_look_around_to_appear() {
        let identity = Identity::of(CoinConfig::olc());
        let mut entries = Vec::new();
        merge_local(&mut entries, &running(), Some(&identity));
        assert_eq!(entries[0].name, "node-0");
        assert_eq!(entries[0].height, Some(40));
    }
}
