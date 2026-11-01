//! This app's own node and the boards as rows of the Network page.
use super::snapshot::NodeView;
use crate::discovery::{Entry, Identity, Reach, Work};
use crate::fleet::BoardRow;
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
