//! Bluetooth Low Energy. A board shows its node id in advertisements and listens for them, which
//! costs little and connects nothing. A connection is made only when the node asks for one, and
//! carries the framing of `olc-link-tcp` over one L2CAP channel, as the program for computers does.
use alloc::collections::VecDeque;
use alloc::format;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec::Vec;
use bt_hci::controller::ExternalController;
use bt_hci::param::LeAdvReportsIter;
use core::cell::{Cell, RefCell};
use embassy_executor::Spawner;
use embassy_futures::select::select;
use embassy_time::{with_timeout, Duration, Instant, Timer};
use esp_hal::peripherals::BT;
use esp_println::println;
use esp_radio::ble::controller::BleConnector;
use olc_node::bluetooth::{node_in_advert, payload, COMPANY, PAYLOAD, PSM};
use olc_node::frame::{encode, Decoder};
use olc_node::{Link, Medium, News, Target};
use static_cell::StaticCell;
use trouble_host::prelude::*;
use trouble_host::scan::Scanner;

/// Connections at once. A board rarely needs more than one for each neighbour without ESP-NOW.
const SLOTS: usize = 3;
const HCI_SLOTS: usize = 20;
const QUEUE_LIMIT: usize = 32;
const SEND_CHECK: Duration = Duration::from_millis(20);
const DIAL_TIMEOUT: Duration = Duration::from_secs(4);
/// Hold a node id this long before telling the node again.
const SEEN_EVERY_MS: u64 = 1_000;
/// Advertising and listening, spaced out so ESP-NOW and mining keep the radio and the processor.
const ADVERTISE_EVERY_MS: u64 = 250;
const SCAN_EVERY_MS: u64 = 400;
const SCAN_WINDOW_MS: u64 = 40;
/// One event every 30 to 50 ms. A quiet link stays up at the cost of a few short exchanges.
const CONNECTION_MS: u64 = 40;
const SUPERVISION_MS: u64 = 4_000;
/// Bytes in one piece. The stack's packets hold 251, less its headers.
const PIECE: usize = 240;

type Controller = ExternalController<BleConnector<'static>, HCI_SLOTS>;
type Pool = DefaultPacketPool;
type BleStack = Stack<'static, Controller, Pool>;
type Conn = Connection<'static, Pool>;

#[derive(Default)]
struct Hub {
    received: VecDeque<(u32, Vec<u8>)>,
    outgoing: [VecDeque<Vec<u8>>; SLOTS],
    open: [bool; SLOTS],
    /// Set to end a connection from the node's side.
    closing: [bool; SLOTS],
    news: VecDeque<News>,
    dials: VecDeque<String>,
    /// When each address was last told to the node.
    told: Vec<([u8; 6], u64)>,
}

type SharedHub = Rc<RefCell<Hub>>;

pub struct BleLink(SharedHub);

// SAFETY: the firmware has one executor and everything sharing this hub runs on it.
// `Link` asks for `Send` because computers share their node between threads.
unsafe impl Send for BleLink {}

impl Link for BleLink {
    fn medium(&self) -> Medium {
        Medium::Bluetooth
    }

    fn send(&mut self, target: Target, message: &[u8]) {
        let mut hub = self.0.borrow_mut();
        let frame = encode(message);
        for slot in 0..SLOTS {
            let wanted = match target {
                Target::Peer(peer) => peer as usize == slot,
                Target::All => true,
                Target::AllExcept(peer) => peer as usize != slot,
            };
            if wanted && hub.open[slot] && hub.outgoing[slot].len() < QUEUE_LIMIT {
                hub.outgoing[slot].push_back(frame.clone());
            }
        }
    }

    fn receive(&mut self) -> Option<(u32, Vec<u8>)> {
        self.0.borrow_mut().received.pop_front()
    }

    fn dial(&mut self, address: &str) {
        self.0.borrow_mut().dials.push_back(address.into());
    }

    fn cancel(&mut self, address: &str) {
        self.0.borrow_mut().dials.retain(|dial| dial != address);
    }

    fn close(&mut self, number: u32) {
        if let Some(slot) = self.0.borrow_mut().closing.get_mut(number as usize) {
            *slot = true;
        }
    }

    fn news(&mut self) -> Option<News> {
        self.0.borrow_mut().news.pop_front()
    }
}

/// Turns what is heard on the air into news for the node.
struct Listener {
    hub: SharedHub,
    me: u64,
    now: Cell<u64>,
}

impl EventHandler for Listener {
    fn on_adv_reports(&self, reports: LeAdvReportsIter) {
        self.now.set(Instant::now().as_millis());
        for report in reports.flatten() {
            let Some(node) = node_in_advert(report.data).filter(|node| *node != self.me) else {
                continue;
            };
            let address = report.addr.into_inner();
            let now = self.now.get();
            let mut hub = self.hub.borrow_mut();
            let full = hub.told.len() >= 16;
            match hub.told.iter_mut().find(|(known, _)| *known == address) {
                Some((_, at)) if now.saturating_sub(*at) < SEEN_EVERY_MS => continue,
                Some((_, at)) => *at = now,
                None if full => continue,
                None => hub.told.push((address, now)),
            }
            hub.news.push_back(News::Seen { node, address: text_of(&address) });
        }
    }
}

fn text_of(address: &[u8; 6]) -> String {
    let [a, b, c, d, e, f] = address;
    format!("{a:02x}:{b:02x}:{c:02x}:{d:02x}:{e:02x}:{f:02x}")
}

fn address_of(text: &str) -> Option<[u8; 6]> {
    let mut bytes = [0u8; 6];
    let mut parts = text.split(':');
    for byte in &mut bytes {
        *byte = u8::from_str_radix(parts.next()?, 16).ok()?;
    }
    Some(bytes)
}

fn take_slot(hub: &SharedHub) -> Option<usize> {
    let mut hub = hub.borrow_mut();
    let slot = hub.open.iter().position(|open| !open)?;
    hub.open[slot] = true;
    hub.closing[slot] = false;
    Some(slot)
}

fn free_slot(hub: &SharedHub, slot: usize) {
    let mut hub = hub.borrow_mut();
    hub.open[slot] = false;
    hub.outgoing[slot].clear();
    hub.news.push_back(News::Closed { number: slot as u32 });
}

/// Moves frames both ways until the far end goes or the node closes this slot.
async fn serve(stack: &'static BleStack, channel: L2capChannel<'static, Pool>, hub: &SharedHub, slot: usize) {
    let (mut writer, mut reader) = channel.split();
    let hearing = async {
        let mut decoder = Decoder::default();
        let mut bytes = [0u8; PIECE + 16];
        loop {
            let Ok(size) = reader.receive(stack, &mut bytes).await else {
                return;
            };
            decoder.push(&bytes[..size]);
            while let Some(message) = decoder.next_message() {
                hub.borrow_mut().received.push_back((slot as u32, message));
            }
            if decoder.is_broken() {
                return;
            }
        }
    };
    let saying = async {
        loop {
            if hub.borrow().closing[slot] {
                return;
            }
            let frame = hub.borrow_mut().outgoing[slot].pop_front();
            let Some(frame) = frame else {
                Timer::after(SEND_CHECK).await;
                continue;
            };
            for piece in frame.chunks(PIECE) {
                if writer.send(stack, piece).await.is_err() {
                    return;
                }
            }
        }
    };
    select(hearing, saying).await;
}

/// Holds one connection the node dialled or a neighbour opened.
#[embassy_executor::task(pool_size = SLOTS)]
async fn connection(stack: &'static BleStack, hub: SharedHub, conn: Conn, slot: usize, dialled: Option<String>) {
    // A computer splits what it writes at this size, and it must fit one read.
    let config = L2capChannelConfig { mtu: Some(PIECE as u16 + 16), ..Default::default() };
    let opened = match dialled {
        Some(_) => with_timeout(DIAL_TIMEOUT, async { L2capChannel::create(stack, &conn, PSM, &config).await.ok() }).await,
        None => {
            let listening = L2capChannel::listen(stack, &conn);
            let accepted = with_timeout(DIAL_TIMEOUT, listening.accept(&config)).await;
            accepted.map(Result::ok)
        }
    };
    match opened {
        Ok(Some(channel)) => {
            hub.borrow_mut().news.push_back(News::Opened { number: slot as u32, address: dialled });
            serve(stack, channel, &hub, slot).await;
            free_slot(&hub, slot);
        }
        _ => {
            hub.borrow_mut().open[slot] = false;
            if let Some(address) = dialled {
                hub.borrow_mut().news.push_back(News::Failed { address });
            }
        }
    }
    conn.disconnect();
}

/// Shows this board, and takes whoever connects to it.
#[embassy_executor::task]
async fn show_self(stack: &'static BleStack, hub: SharedHub, spawner: Spawner, node: u64) {
    let mut peripheral = stack.peripheral();
    let mut advert = [0u8; 31];
    let data = payload(node);
    let size: Result<usize, _> = AdStructure::encode_slice(
        &[
            AdStructure::Flags(LE_GENERAL_DISCOVERABLE | BR_EDR_NOT_SUPPORTED),
            AdStructure::ManufacturerSpecificData { company_identifier: COMPANY, payload: &data[..PAYLOAD] },
        ],
        &mut advert,
    );
    let size = size.unwrap_or(0);
    let parameters = AdvertisementParameters {
        interval_min: Duration::from_millis(ADVERTISE_EVERY_MS),
        interval_max: Duration::from_millis(ADVERTISE_EVERY_MS + 50),
        ..Default::default()
    };
    loop {
        let Some(slot) = take_slot(&hub) else {
            Timer::after(Duration::from_secs(1)).await;
            continue;
        };
        let shown = peripheral
            .advertise(
                &parameters,
                Advertisement::ConnectableScannableUndirected { adv_data: &advert[..size], scan_data: &[] },
            )
            .await;
        let conn = match shown {
            Ok(advertiser) => advertiser.accept().await.ok(),
            Err(_) => None,
        };
        match conn {
            Some(conn) => spawner.spawn(connection(stack, hub.clone(), conn, slot, None).expect("one task per slot")),
            None => {
                hub.borrow_mut().open[slot] = false;
                Timer::after(Duration::from_secs(1)).await;
            }
        }
    }
}

fn parameters() -> RequestedConnParams {
    RequestedConnParams {
        min_connection_interval: Duration::from_millis(CONNECTION_MS - 10),
        max_connection_interval: Duration::from_millis(CONNECTION_MS + 10),
        max_latency: 0,
        min_event_length: Duration::from_millis(0),
        max_event_length: Duration::from_millis(0),
        supervision_timeout: Duration::from_millis(SUPERVISION_MS),
    }
}

/// Listens for advertisements, and dials when the node says so.
#[embassy_executor::task]
async fn dial_on_order(stack: &'static BleStack, hub: SharedHub, spawner: Spawner) {
    let timing = ScanConfig {
        active: false,
        interval: Duration::from_millis(SCAN_EVERY_MS),
        window: Duration::from_millis(SCAN_WINDOW_MS),
        ..Default::default()
    };
    let mut scanner = Scanner::new(stack.central());
    loop {
        let session = scanner.scan(&timing).await;
        loop {
            let waiting = hub.borrow().dials.front().cloned();
            if waiting.is_some() {
                break;
            }
            Timer::after(SEND_CHECK).await;
        }
        drop(session);
        let mut central = scanner.into_inner();
        let address = hub.borrow_mut().dials.pop_front().unwrap_or_default();
        let slot = take_slot(&hub);
        let conn = match (address_of(&address), slot) {
            (Some(bytes), Some(_)) => {
                let target = [Address::random(bytes)];
                let config = ConnectConfig {
                    scan_config: ScanConfig { filter_accept_list: &target, ..timing_for_connecting() },
                    connect_params: parameters(),
                };
                with_timeout(DIAL_TIMEOUT, central.connect(&config)).await.ok().and_then(Result::ok)
            }
            _ => None,
        };
        match (conn, slot) {
            (Some(conn), Some(slot)) => {
                let task = connection(stack, hub.clone(), conn, slot, Some(address));
                spawner.spawn(task.expect("one task per slot"));
            }
            _ => {
                if let Some(slot) = slot {
                    hub.borrow_mut().open[slot] = false;
                }
                hub.borrow_mut().news.push_back(News::Failed { address });
            }
        }
        scanner = Scanner::new(central);
    }
}

/// Connecting listens harder than looking around does, for the moment it takes.
fn timing_for_connecting() -> ScanConfig<'static> {
    ScanConfig {
        interval: Duration::from_millis(60),
        window: Duration::from_millis(30),
        ..Default::default()
    }
}

#[embassy_executor::task]
async fn run_host(stack: &'static BleStack, listener: Listener) {
    let mut runner = stack.runner();
    loop {
        if let Err(error) = runner.run_with_handler(&listener).await {
            println!("bluetooth stopped: {error:?}");
            Timer::after(Duration::from_secs(5)).await;
        }
    }
}

/// Starts Bluetooth. Costs memory only on a board that asked for it.
pub fn start(spawner: Spawner, device: BT<'static>, node: u64, address: [u8; 6]) -> BleLink {
    let config = esp_radio::ble::Config::default().with_max_connections(SLOTS as u8);
    let connector = BleConnector::new(device, config).expect("Bluetooth starts");
    let controller = Controller::new(connector);
    static RESOURCES: StaticCell<HostResources<Controller, Pool, SLOTS, SLOTS>> = StaticCell::new();
    static STACK: StaticCell<BleStack> = StaticCell::new();
    let mut random = address;
    // A static random address has its top two bits set, and the top byte comes last here.
    random[5] |= 0b1100_0000;
    let stack = trouble_host::new(controller, RESOURCES.init(HostResources::new()))
        .set_random_address(Address::random(random))
        .register_l2cap_spsm(PSM)
        .build();
    let stack: &'static BleStack = STACK.init(stack);
    let hub = SharedHub::default();
    let listener = Listener { hub: hub.clone(), me: node, now: Cell::new(0) };
    spawner.spawn(run_host(stack, listener).expect("one host task"));
    spawner.spawn(show_self(stack, hub.clone(), spawner, node).expect("one advertising task"));
    spawner.spawn(dial_on_order(stack, hub.clone(), spawner).expect("one dialling task"));
    println!("bluetooth on at {}", text_of(&random));
    BleLink(hub)
}

