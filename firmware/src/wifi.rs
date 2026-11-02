//! Wi-Fi. A board can host an open network of its own, so a computer or phone can reach it with
//! no router. A board can also join a router, where it says who it is by broadcast and dials the
//! nodes it hears. Nodes speak the same TCP framing as `olc-link-tcp`:
//! `olc node --connect 192.168.4.1:7000`.
use crate::store::Settings;
use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::rc::Rc;
use alloc::vec;
use alloc::vec::Vec;
use core::cell::RefCell;
use core::net::{Ipv4Addr, SocketAddrV4};
use alloc::string::String;
use edge_dhcp::io::{self, DEFAULT_SERVER_PORT};
use edge_dhcp::server::{Server, ServerOptions};
use edge_nal::UdpBind;
use edge_nal_embassy::{Udp, UdpBuffers};
use embassy_executor::Spawner;
use embassy_net::tcp::TcpSocket;
use embassy_net::udp::{PacketMetadata, UdpSocket};
use embassy_net::{IpAddress, Ipv4Cidr, Runner as NetRunner, Stack, StackResources, StaticConfigV4};
use embassy_time::{with_timeout, Duration, Timer};
use esp_hal::peripherals::WIFI;
use esp_radio::esp_now::EspNow;
use esp_radio::wifi::ap::AccessPointConfig;
use esp_radio::wifi::sta::StationConfig;
use esp_radio::wifi::{
    AuthenticationMethodConfig, Config, Interface, PowerSaveMode, WifiController,
};
use olc_node::frame::{encode, Decoder};
use olc_node::lan;
use olc_node::{Link, Medium, News, Target};
use static_cell::StaticCell;

pub const CHANNEL: u8 = 1;
pub const PORT: u16 = 7000;
pub const OWN_ADDRESS: Ipv4Addr = Ipv4Addr::new(192, 168, 4, 1);

const SLOTS: usize = 5;
/// Connections on the board's own network. The rest are on the router's.
const OWN_SLOTS: usize = 2;
/// Every stack also holds a DNS socket, which `edge-nal-embassy` turns on, and a DHCP one: the
/// server's on the board's own network, the client's on the router's. The router's also holds
/// the socket that finds neighbours. A stack with too few panics when a socket opens.
const OWN_SOCKETS: usize = OWN_SLOTS + 2;
const ROUTER_SOCKETS: usize = SLOTS - OWN_SLOTS + 3;
const SOCKET_BUFFER: usize = 2_048;
const QUEUE_LIMIT: usize = 32;
const SEND_CHECK: Duration = Duration::from_millis(20);
const RETRY: Duration = Duration::from_secs(3);
/// The router is looked for less often the longer it stays away.
const LONGEST_RETRY: Duration = Duration::from_secs(60);
const DIAL_TIMEOUT: Duration = Duration::from_secs(5);

/// What the connection tasks share with the node.
#[derive(Default)]
struct Hub {
    received: VecDeque<(u32, Vec<u8>)>,
    outgoing: [VecDeque<Vec<u8>>; SLOTS],
    open: [bool; SLOTS],
    /// Set to end a connection from the node's side.
    closing: [bool; SLOTS],
    news: VecDeque<News>,
    /// Addresses the node asked this board to dial, oldest first.
    dials: VecDeque<String>,
}

type SharedHub = Rc<RefCell<Hub>>;

pub struct WifiLink(SharedHub);

// SAFETY: the firmware has one executor and everything sharing this hub runs on it.
// `Link` asks for `Send` because computers share their node between threads.
unsafe impl Send for WifiLink {}

impl Link for WifiLink {
    fn medium(&self) -> Medium {
        Medium::Wifi
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
        let mut hub = self.0.borrow_mut();
        if hub.dials.len() < QUEUE_LIMIT && !hub.dials.iter().any(|dial| dial == address) {
            hub.dials.push_back(address.into());
        }
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

/// The two networks a board is on, for showing their addresses.
pub struct Network {
    pub own_name: alloc::string::String,
    pub router: Stack<'static>,
    pub joins_router: bool,
}

impl Network {
    pub fn router_address(&self) -> Option<Ipv4Addr> {
        self.router
            .config_v4()
            .map(|config| config.address.address())
    }
}

#[embassy_executor::task(pool_size = 2)]
async fn run_stack(mut runner: NetRunner<'static, Interface>) {
    runner.run().await
}

#[embassy_executor::task]
async fn stay_on_router(mut controller: WifiController<'static>) {
    let mut wait = RETRY;
    loop {
        if controller.connect_async().await.is_ok() {
            wait = RETRY;
            let _ = controller.wait_for_disconnect_async().await;
        }
        Timer::after(wait).await;
        wait = (wait + wait).min(LONGEST_RETRY);
    }
}

/// Hands out addresses on the board's own network. It offers no gateway, so a computer that
/// joins keeps its internet connection on its other interface.
#[embassy_executor::task]
async fn hand_out_addresses(stack: Stack<'static>) {
    let buffers = UdpBuffers::<1, 1024, 1024, 4>::new();
    let udp = Udp::new(stack, &buffers);
    let local = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, DEFAULT_SERVER_PORT);
    let Ok(mut socket) = udp.bind(local.into()).await else {
        return;
    };
    let mut buffer = vec![0u8; 1_500];
    loop {
        let _ = io::server::run(
            &mut Server::<_, 8>::new_with_et(OWN_ADDRESS),
            &ServerOptions::new(OWN_ADDRESS, None),
            &mut socket,
            &mut buffer,
        )
        .await;
        Timer::after(RETRY).await;
    }
}

/// Dials the next address the node asks for. Gives it back when connected.
async fn dial_once(socket: &mut TcpSocket<'_>, stack: Stack<'static>, hub: &SharedHub) -> Option<String> {
    let address = loop {
        let next = hub.borrow_mut().dials.pop_front();
        match next {
            Some(address) => break address,
            None => Timer::after(SEND_CHECK).await,
        }
    };
    let target: Option<SocketAddrV4> = address.parse().ok();
    let reached = with_timeout(DIAL_TIMEOUT, async {
        stack.wait_config_up().await;
        match target {
            Some(target) => socket.connect((*target.ip(), target.port())).await.is_ok(),
            None => false,
        }
    });
    if reached.await.unwrap_or(false) {
        return Some(address);
    }
    hub.borrow_mut().news.push_back(News::Failed { address });
    None
}

/// Says this board is here on the router's network, and tells the node who else is. Only nodes on
/// the same router hear it, and they dial the address it came from.
#[embassy_executor::task]
async fn find_neighbours(stack: Stack<'static>, hub: SharedHub, me: u64) {
    let mut receive_meta = [PacketMetadata::EMPTY; 4];
    let mut receive = [0u8; 512];
    let mut send_meta = [PacketMetadata::EMPTY; 2];
    let mut send = [0u8; 128];
    let mut socket = UdpSocket::new(stack, &mut receive_meta, &mut receive, &mut send_meta, &mut send);
    if socket.bind(PORT).is_err() {
        return;
    }
    let said = lan::announcement(me);
    let every = Duration::from_millis(lan::SAY_EVERY_MS);
    let mut bytes = [0u8; 64];
    loop {
        stack.wait_config_up().await;
        let _ = socket.send_to(&said, (Ipv4Addr::BROADCAST, PORT)).await;
        let until = embassy_time::Instant::now() + every;
        while let Ok(Ok((size, from))) =
            with_timeout(until.saturating_duration_since(embassy_time::Instant::now()), socket.recv_from(&mut bytes)).await
        {
            let IpAddress::Ipv4(ip) = from.endpoint.addr else {
                continue;
            };
            let sender = SocketAddrV4::new(ip, from.endpoint.port);
            let Some((node, address)) = lan::heard(&bytes[..size.min(bytes.len())], sender, me) else {
                continue;
            };
            let mut hub = hub.borrow_mut();
            if hub.news.len() < QUEUE_LIMIT {
                hub.news.push_back(News::Seen { node, address });
            }
        }
    }
}

/// One TCP connection, accepted on `PORT` or dialled on the node's order when `dials` is set.
#[embassy_executor::task(pool_size = SLOTS)]
async fn connection(stack: Stack<'static>, hub: SharedHub, slot: usize, dials: bool) {
    let receive = Box::leak(vec![0u8; SOCKET_BUFFER].into_boxed_slice());
    let send = Box::leak(vec![0u8; SOCKET_BUFFER].into_boxed_slice());
    loop {
        let mut socket = TcpSocket::new(stack, &mut *receive, &mut *send);
        socket.set_timeout(Some(Duration::from_secs(30)));
        socket.set_keep_alive(Some(Duration::from_secs(10)));
        let (connected, address) = if dials {
            let address = dial_once(&mut socket, stack, &hub).await;
            (address.is_some(), address)
        } else {
            (socket.accept(PORT).await.is_ok(), None)
        };
        if connected {
            {
                let mut hub = hub.borrow_mut();
                hub.open[slot] = true;
                hub.closing[slot] = false;
                hub.news.push_back(News::Opened { number: slot as u32, address });
            }
            serve(&mut socket, &hub, slot).await;
            let mut hub = hub.borrow_mut();
            hub.open[slot] = false;
            hub.outgoing[slot].clear();
            hub.news.push_back(News::Closed { number: slot as u32 });
        }
        socket.abort();
        if !dials {
            Timer::after(RETRY).await;
        }
    }
}

async fn serve(socket: &mut TcpSocket<'_>, hub: &SharedHub, slot: usize) {
    let mut decoder = Decoder::at_most(crate::links::most_message());
    let mut bytes = [0u8; 512];
    loop {
        if hub.borrow().closing[slot] {
            return;
        }
        loop {
            let frame = hub.borrow_mut().outgoing[slot].pop_front();
            let Some(frame) = frame else { break };
            let mut rest = frame.as_slice();
            while !rest.is_empty() {
                match socket.write(rest).await {
                    Ok(0) | Err(_) => return,
                    Ok(sent) => rest = &rest[sent..],
                }
            }
        }
        // The node takes a few messages a turn. What it has not reached stays on the socket.
        if hub.borrow().received.len() >= QUEUE_LIMIT {
            Timer::after(SEND_CHECK).await;
            continue;
        }
        match with_timeout(SEND_CHECK, socket.read(&mut bytes)).await {
            Ok(Ok(0) | Err(_)) => return,
            Ok(Ok(size)) => {
                decoder.push(&bytes[..size]);
                while let Some(message) = decoder.next_message() {
                    hub.borrow_mut().received.push_back((slot as u32, message));
                }
                if decoder.is_broken() {
                    return;
                }
            }
            Err(_) => {}
        }
    }
}

fn station_config(settings: &Settings) -> Option<StationConfig> {
    let ssid = settings.ssid.as_str().try_into().ok()?;
    let authentication = if settings.password.is_empty() {
        AuthenticationMethodConfig::Open
    } else {
        AuthenticationMethodConfig::Wpa2Personal(settings.password.as_str().try_into().ok()?)
    };
    Some(
        StationConfig::default()
            .with_ssid(ssid)
            .with_authentication(authentication),
    )
}

/// Starts Wi-Fi with both networks and everything that listens on them.
pub fn start(
    spawner: Spawner,
    device: WIFI<'static>,
    settings: &Settings,
    own_name: &str,
    power_dbm: i8,
    seed: u64,
    me: u64,
) -> (EspNow, WifiLink, Network) {
    let mut controller = WifiController::new(device, Default::default()).expect("Wi-Fi starts");
    if settings.bluetooth_on() {
        // The two radios share one antenna, and Wi-Fi has to let go of it between frames.
        let _ = controller.set_power_saving(PowerSaveMode::Minimum);
    }
    let joins_router = !settings.ssid.is_empty();
    let station = station_config(settings).unwrap_or_default();
    let name = own_name.try_into().expect("the name fits an SSID");
    let access_point = AccessPointConfig::default()
        .with_ssid(name)
        .with_channel(CHANNEL);
    // Boards reach each other over ESP-NOW, so this comes up only when asked for.
    let config = if settings.own_network == "on" {
        Config::AccessPointStation(station, access_point)
    } else {
        Config::Station(station)
    };
    controller
        .set_config(&config)
        .expect("the Wi-Fi settings are valid");
    // In quarters of a dBm, and only possible once Wi-Fi runs.
    let _ = controller.set_max_tx_power(power_dbm.saturating_mul(4));
    let espnow = controller.esp_now();
    let _ = espnow.set_channel(CHANNEL);

    static OWN_RESOURCES: StaticCell<StackResources<OWN_SOCKETS>> = StaticCell::new();
    static ROUTER_RESOURCES: StaticCell<StackResources<ROUTER_SOCKETS>> = StaticCell::new();
    let own_config = embassy_net::Config::ipv4_static(StaticConfigV4 {
        address: Ipv4Cidr::new(OWN_ADDRESS, 24),
        gateway: None,
        dns_servers: Default::default(),
    });
    let (own, own_runner) = embassy_net::new(
        Interface::access_point(),
        own_config,
        OWN_RESOURCES.init(StackResources::new()),
        seed,
    );
    let (router, router_runner) = embassy_net::new(
        Interface::station(),
        embassy_net::Config::dhcpv4(Default::default()),
        ROUTER_RESOURCES.init(StackResources::new()),
        seed ^ 1,
    );
    spawner.spawn(run_stack(own_runner).expect("one task per stack"));
    spawner.spawn(run_stack(router_runner).expect("one task per stack"));
    spawner.spawn(hand_out_addresses(own).expect("one address task"));
    let hub = SharedHub::default();
    if joins_router {
        spawner.spawn(stay_on_router(controller).expect("one router task"));
        spawner.spawn(find_neighbours(router, hub.clone(), me).expect("one finding task"));
    }
    // The last slot dials whoever the node asks for: a typed address or a neighbour it heard.
    for slot in 0..SLOTS {
        let stack = if slot < OWN_SLOTS { own } else { router };
        let dials = slot == SLOTS - 1;
        spawner.spawn(connection(stack, hub.clone(), slot, dials).expect("a task per slot"));
    }
    let network = Network {
        own_name: if settings.own_network == "on" {
            own_name.into()
        } else {
            Default::default()
        },
        router,
        joins_router,
    };
    (espnow, WifiLink(hub), network)
}
