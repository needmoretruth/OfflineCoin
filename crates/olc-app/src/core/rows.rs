//! This app's own node and the boards as rows of the Network page.
use super::snapshot::NodeView;
use crate::discovery::{Entry, Identity, Reach, Work};
use crate::fleet::BoardRow;
use crate::local::LocalView;
use olc_core::Hash;
use olc_node::Hello;

const BOARD_KIND: &str = "esp32-s3";

pub fn hostname() -> String {
    std::fs::read_to_string("/proc/sys/kernel/hostname")
        .map(|name| name.trim().to_string())
        .unwrap_or_default()
}

/// What this program runs on, in the words a node gives for itself.
pub fn device_kind() -> String {
    let model = std::fs::read_to_string("/sys/firmware/devicetree/base/model").unwrap_or_default();
    match model.contains("Raspberry") {
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
    }
}

/// A node this app runs reads live, rather than as it stood at the last look around.
pub fn refresh_local(entry: &mut Entry, nodes: &[LocalView], coin: Option<&Identity>) {
    // Another program's node on this computer, on a coin of its own, stays as it answered.
    let same = |found: &Identity| coin.is_some_and(|coin| coin.genesis == found.genesis);
    if !entry.identity.as_ref().is_some_and(same) {
        return;
    }
    let port = entry.address.as_deref().and_then(|address| address.rsplit(':').next());
    let port = port.and_then(|port| port.parse::<u16>().ok());
    let live = nodes
        .iter()
        .filter_map(|node| node.status.as_ref())
        .find(|status| entry.reach == Reach::ThisComputer && status.port == port && port.is_some());
    if let Some(status) = live {
        entry.height = Some(status.height);
        entry.work_done = status.work.parse().ok().or(entry.work_done);
        entry.tip = hash_of(&status.tip).or(entry.tip);
        entry.work = match status.mining {
            true => Work::Mining(status.hashrate),
            false => Work::Idle,
        };
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

pub fn reach_of_board(row: &BoardRow) -> Reach {
    match (row.on_cable, &row.through) {
        (true, _) => Reach::Usb,
        (false, through) => Reach::Radio(through.clone()),
    }
}

/// `said` is what the board told about itself. Until it has, its coin stays unknown.
pub fn board_entry(row: &BoardRow, said: Option<Hello>) -> Entry {
    let beacon = &row.seen.beacon;
    Entry {
        reach: reach_of_board(row),
        address: None,
        via: None,
        device: BOARD_KIND.into(),
        name: beacon.name.clone(),
        identity: said.as_ref().map(|hello| Identity::of(hello.coin.clone())),
        tip: Some(beacon.tip),
        height: Some(beacon.height),
        work_done: said.as_ref().map(|hello| hello.work),
        build: beacon.build.clone(),
        work: match (row.seen.online, beacon.offline, beacon.mining) {
            (false, _, _) => Work::Silent,
            (true, true, _) => Work::Offline,
            (true, false, true) => Work::Mining(u64::from(beacon.hashrate)),
            (true, false, false) => Work::Idle,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local::{ChildStatus, Enforcement, Preset};
    use olc_core::CoinConfig;

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
    fn a_running_node_reads_live_only_on_this_apps_coin() {
        let ours = CoinConfig::olc();
        let mut other = CoinConfig::olc();
        other.message = "Another genesis".into();
        let identity = Identity::of(ours.clone());

        let mut entry = found(&ours);
        refresh_local(&mut entry, &running(), Some(&identity));
        assert_eq!(entry.height, Some(40));

        let mut entry = found(&other);
        refresh_local(&mut entry, &running(), Some(&identity));
        assert_eq!(entry.height, Some(3));
    }
}
