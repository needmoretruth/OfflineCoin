//! Bluetooth for a Linux computer. It listens for boards showing their node id, and when the node
//! asks, opens one L2CAP channel carrying the same frames as TCP. It never advertises, because a
//! computer dialling out is all that is needed and a board is what it talks to.
#![cfg(feature = "bluez")]

use bluer::l2cap::{self, SocketAddr};
use bluer::{Adapter, AdapterEvent, Address, AddressType, DeviceEvent, DiscoveryFilter, DiscoveryTransport, Session};
use futures::{pin_mut, Stream, StreamExt};
use olc_node::bluetooth::{node_of, COMPANY, PSM};
use olc_node::frame::{encode, Decoder};
use olc_node::{Link, Medium, News, Target};
use std::collections::BTreeMap;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use tokio::time::{timeout, Instant};

const DIAL_TIMEOUT: Duration = Duration::from_secs(6);
const READ_SIZE: usize = 1024;
/// What one write may hold, as the boards accept it.
const PIECE: usize = 240;
/// One sign of life for each advertiser this often is plenty.
const SEEN_EVERY: Duration = Duration::from_secs(1);
/// How soon listening restarts after Bluetooth came back.
const LISTEN_AGAIN: Duration = Duration::from_secs(5);

type Writers = Arc<Mutex<BTreeMap<u32, UnboundedSender<Vec<u8>>>>>;

/// What the Bluetooth thread shares with its tasks.
#[derive(Clone)]
struct Shared {
    writers: Writers,
    received: Sender<(u32, Vec<u8>)>,
    news: Sender<News>,
    next: Arc<Mutex<u32>>,
}

pub struct BleLink {
    writers: Writers,
    orders: UnboundedSender<Order>,
    received: Receiver<(u32, Vec<u8>)>,
    news: Receiver<News>,
}

enum Order {
    Dial(String),
}

impl BleLink {
    /// Starts Bluetooth on the computer's first adapter, and fails if there is none.
    pub fn start(me: u64) -> Result<Self, String> {
        let (received_sender, received) = channel();
        let (news_sender, news) = channel();
        let (orders, order_receiver) = unbounded_channel();
        let writers = Writers::default();
        let shared = Shared {
            writers: writers.clone(),
            received: received_sender,
            news: news_sender,
            next: Arc::default(),
        };
        let (ready, answer) = channel();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| error.to_string())?;
        thread::spawn(move || {
            let local = tokio::task::LocalSet::new();
            local.block_on(&runtime, run(shared, order_receiver, me, ready));
        });
        answer.recv().map_err(|error| error.to_string())??;
        Ok(Self { writers, orders, received, news })
    }
}

impl Link for BleLink {
    fn medium(&self) -> Medium {
        Medium::Bluetooth
    }

    fn send(&mut self, target: Target, message: &[u8]) {
        let Ok(writers) = self.writers.lock() else {
            return;
        };
        let frame = encode(message);
        for (number, writer) in writers.iter() {
            let wanted = match target {
                Target::Peer(peer) => peer == *number,
                Target::All => true,
                Target::AllExcept(peer) => peer != *number,
            };
            if wanted {
                let _ = writer.send(frame.clone());
            }
        }
    }

    fn receive(&mut self) -> Option<(u32, Vec<u8>)> {
        self.received.try_recv().ok()
    }

    fn dial(&mut self, address: &str) {
        let _ = self.orders.send(Order::Dial(address.into()));
    }

    fn close(&mut self, number: u32) {
        if let Ok(mut writers) = self.writers.lock() {
            writers.remove(&number);
        }
    }

    fn news(&mut self) -> Option<News> {
        self.news.try_recv().ok()
    }
}

async fn open_adapter() -> bluer::Result<Adapter> {
    let session = Session::new().await?;
    let adapter = session.default_adapter().await?;
    adapter.set_powered(true).await?;
    Ok(adapter)
}

/// Starts listening. A person turning Bluetooth off and on ends this, so it is repeated.
async fn listen(adapter: &Adapter) -> bluer::Result<impl Stream<Item = AdapterEvent>> {
    let filter = DiscoveryFilter {
        transport: DiscoveryTransport::Le,
        duplicate_data: true,
        ..Default::default()
    };
    adapter.set_discovery_filter(filter).await?;
    adapter.discover_devices_with_changes().await
}

async fn run(shared: Shared, mut orders: UnboundedReceiver<Order>, me: u64, ready: Sender<Result<(), String>>) {
    let adapter = match open_adapter().await {
        Ok(adapter) => adapter,
        Err(error) => {
            let _ = ready.send(Err(format!("Bluetooth: {error}")));
            return;
        }
    };
    let _ = ready.send(Ok(()));
    loop {
        let Ok(events) = listen(&adapter).await else {
            tokio::time::sleep(LISTEN_AGAIN).await;
            while let Ok(order) = orders.try_recv() {
                let Order::Dial(address) = order;
                let _ = shared.news.send(News::Failed { address });
            }
            continue;
        };
        pin_mut!(events);
        let mut check = tokio::time::interval(LISTEN_AGAIN);
        loop {
            tokio::select! {
                event = events.next() => match event {
                    Some(AdapterEvent::DeviceAdded(address)) => {
                        tokio::task::spawn_local(watch(adapter.clone(), address, me, shared.news.clone()));
                    }
                    Some(_) => {}
                    None => break,
                },
                order = orders.recv() => match order {
                    Some(Order::Dial(address)) => {
                        tokio::task::spawn_local(dial(shared.clone(), address));
                    }
                    None => return,
                },
                _ = check.tick() => {
                    if !adapter.is_discovering().await.unwrap_or(false) {
                        break;
                    }
                }
            }
        }
    }
}

/// Follows one device while it stays around, and tells the node each time it shows itself.
async fn watch(adapter: Adapter, address: Address, me: u64, news: Sender<News>) -> bluer::Result<()> {
    let device = adapter.device(address)?;
    let changes = device.events().await?;
    pin_mut!(changes);
    let mut told: Option<Instant> = None;
    let mut known = None;
    loop {
        if known.is_none() {
            let data = device.manufacturer_data().await?.unwrap_or_default();
            known = data.get(&COMPANY).and_then(|payload| node_of(payload)).filter(|node| *node != me);
        }
        if let Some(node) = known {
            if told.is_none_or(|at| at.elapsed() >= SEEN_EVERY) {
                told = Some(Instant::now());
                let address = address.to_string();
                if news.send(News::Seen { node, address }).is_err() {
                    return Ok(());
                }
            }
        }
        let Some(DeviceEvent::PropertyChanged(_)) = changes.next().await else {
            return Ok(());
        };
    }
}

/// One try. The answer comes back as news.
async fn dial(shared: Shared, address: String) {
    let target = address.parse().ok().map(|address| SocketAddr::new(address, AddressType::LeRandom, PSM));
    let stream = match target {
        Some(target) => timeout(DIAL_TIMEOUT, l2cap::Stream::connect(target)).await.ok().and_then(Result::ok),
        None => None,
    };
    let Some(stream) = stream else {
        let _ = shared.news.send(News::Failed { address });
        return;
    };
    let number = {
        let mut next = shared.next.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        *next += 1;
        *next
    };
    let (writer, frames) = unbounded_channel();
    if let Ok(mut writers) = shared.writers.lock() {
        writers.insert(number, writer);
    }
    let _ = shared.news.send(News::Opened { number, address: Some(address) });
    serve(stream, frames, &shared, number).await;
    if let Ok(mut writers) = shared.writers.lock() {
        writers.remove(&number);
    }
    let _ = shared.news.send(News::Closed { number });
}

/// Moves frames both ways until the far end goes or the node closes this one.
async fn serve(stream: l2cap::Stream, mut frames: UnboundedReceiver<Vec<u8>>, shared: &Shared, number: u32) {
    let (mut reader, mut writer) = stream.into_split();
    let hearing = async {
        let mut decoder = Decoder::default();
        let mut bytes = vec![0u8; READ_SIZE];
        loop {
            let size = reader.read(&mut bytes).await.unwrap_or(0);
            if size == 0 {
                return;
            }
            decoder.push(&bytes[..size]);
            while let Some(message) = decoder.next_message() {
                if shared.received.send((number, message)).is_err() {
                    return;
                }
            }
            if decoder.is_broken() {
                return;
            }
        }
    };
    let saying = async {
        while let Some(frame) = frames.recv().await {
            for piece in frame.chunks(PIECE) {
                if writer.write_all(piece).await.is_err() {
                    return;
                }
            }
        }
    };
    tokio::select! {
        () = hearing => {}
        () = saying => {}
    }
}
