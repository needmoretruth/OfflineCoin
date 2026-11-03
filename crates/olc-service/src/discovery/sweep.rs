//! Looks at every address worth trying, a few at a time. The firewall of this computer may drop
//! whatever comes in, so every look starts here and goes out.
use super::probe::ask;
use olc_node::Hello;
use std::collections::BTreeSet;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const NODE_PORT: u16 = 7000;
/// `olc devnet` and a few nodes on one computer sit here.
const LOCAL_PORTS: [(u16, u16); 2] = [(7000, 7012), (7100, 7110)];
const NEAR_WAIT: Duration = Duration::from_millis(350);
const FAR_WAIT: Duration = Duration::from_secs(3);
const WORKERS: usize = 32;
const LOOK_EVERY: Duration = Duration::from_secs(45);
const POLL: Duration = Duration::from_millis(200);

/// What the next look covers. The app keeps it current.
#[derive(Clone, Debug, Default)]
pub struct ScanPlan {
    pub lan: bool,
    pub ports: Vec<u16>,
    /// Addresses to try whatever else happens: added by a person, or passed on by a node.
    pub addresses: Vec<String>,
}

#[derive(Default)]
struct Shared {
    plan: Mutex<ScanPlan>,
    found: Mutex<Vec<(String, Hello)>>,
    silent: Mutex<Vec<String>>,
    wake: AtomicBool,
    running: AtomicBool,
    tried: AtomicUsize,
    finished: Mutex<Option<Instant>>,
    subnets: Mutex<Vec<String>>,
}

pub struct Scanner {
    shared: Arc<Shared>,
}

/// The 254 neighbours of an address. A wider network is left at this: walking all of it would
/// take minutes and most of it is empty.
fn neighbours(ip: Ipv4Addr) -> impl Iterator<Item = Ipv4Addr> {
    let [a, b, c, own] = ip.octets();
    (1..=254u8)
        .filter(move |last| *last != own)
        .map(move |last| Ipv4Addr::new(a, b, c, last))
}

/// This computer's address on each local network.
pub fn local_networks() -> Vec<Ipv4Addr> {
    let Ok(interfaces) = if_addrs::get_if_addrs() else {
        return Vec::new();
    };
    interfaces
        .into_iter()
        .filter(|interface| !interface.is_loopback())
        .filter(|interface| {
            // Containers and their bridges hold no nodes unless someone put them there, and a
            // user who did can add the address.
            !["docker", "veth", "br-", "podman"]
                .iter()
                .any(|start| interface.name.starts_with(start))
        })
        .filter_map(|interface| match interface.ip() {
            IpAddr::V4(ip) if !ip.is_link_local() => Some(ip),
            _ => None,
        })
        .collect()
}

fn with_port(address: &str) -> String {
    if address.rsplit_once(':').is_some_and(|(_, port)| port.parse::<u16>().is_ok()) {
        address.to_string()
    } else {
        format!("{address}:{NODE_PORT}")
    }
}

fn targets(plan: &ScanPlan, networks: &[Ipv4Addr]) -> (Vec<String>, Vec<String>) {
    let mut near = BTreeSet::new();
    for (first, last) in LOCAL_PORTS {
        near.extend((first..last).map(|port| format!("127.0.0.1:{port}")));
    }
    for port in &plan.ports {
        near.insert(format!("127.0.0.1:{port}"));
    }
    if plan.lan {
        let ports: Vec<u16> = std::iter::once(NODE_PORT)
            .chain(plan.ports.iter().copied())
            .collect();
        for ip in networks.iter().flat_map(|ip| neighbours(*ip)) {
            near.extend(ports.iter().map(|port| format!("{ip}:{port}")));
        }
    }
    let far: BTreeSet<String> = plan.addresses.iter().map(|text| with_port(text)).collect();
    let near = near.difference(&far).cloned().collect();
    (near, far.into_iter().collect())
}

impl Shared {
    fn try_all(&self, addresses: &[String], wait: Duration, workers: usize) {
        let next = AtomicUsize::new(0);
        thread::scope(|scope| {
            for _ in 0..workers {
                scope.spawn(|| {
                    while let Some(address) = addresses.get(next.fetch_add(1, Ordering::Relaxed)) {
                        self.tried.fetch_add(1, Ordering::Relaxed);
                        match ask(address, wait) {
                            Some(hello) => {
                                if let Ok(mut found) = self.found.lock() {
                                    found.push((address.clone(), hello));
                                }
                            }
                            None => {
                                if let Ok(mut silent) = self.silent.lock() {
                                    silent.push(address.clone());
                                }
                            }
                        }
                    }
                });
            }
        });
    }

    fn look(&self) {
        self.running.store(true, Ordering::Relaxed);
        self.tried.store(0, Ordering::Relaxed);
        let plan = self.plan.lock().map(|plan| plan.clone()).unwrap_or_default();
        let networks = local_networks();
        if let Ok(mut subnets) = self.subnets.lock() {
            *subnets = networks
                .iter()
                .map(|ip| {
                    let [a, b, c, _] = ip.octets();
                    format!("{a}.{b}.{c}.0/24")
                })
                .collect();
        }
        let (near, far) = targets(&plan, &networks);
        self.try_all(&far, FAR_WAIT, WORKERS.min(far.len().max(1)));
        self.try_all(&near, NEAR_WAIT, WORKERS);
        self.running.store(false, Ordering::Relaxed);
        if let Ok(mut finished) = self.finished.lock() {
            *finished = Some(Instant::now());
        }
    }
}

impl Scanner {
    pub fn start() -> Self {
        let shared = Arc::new(Shared::default());
        let worker = shared.clone();
        thread::spawn(move || {
            // The first look waits for the app to say what to look at.
            while !worker.wake.swap(false, Ordering::Relaxed) {
                thread::sleep(POLL);
            }
            loop {
                worker.look();
                let waited = Instant::now();
                while waited.elapsed() < LOOK_EVERY && !worker.wake.swap(false, Ordering::Relaxed) {
                    thread::sleep(POLL);
                }
            }
        });
        Self { shared }
    }

    pub fn set_plan(&self, plan: ScanPlan) {
        if let Ok(mut slot) = self.shared.plan.lock() {
            *slot = plan;
        }
    }

    pub fn look_now(&self) {
        self.shared.wake.store(true, Ordering::Relaxed);
    }

    /// What answered since the last call.
    pub fn take_found(&self) -> Vec<(String, Hello)> {
        self.shared
            .found
            .lock()
            .map(|mut found| found.drain(..).collect())
            .unwrap_or_default()
    }

    /// What stayed quiet since the last call.
    pub fn take_silent(&self) -> Vec<String> {
        self.shared
            .silent
            .lock()
            .map(|mut silent| silent.drain(..).collect())
            .unwrap_or_default()
    }

    pub fn running(&self) -> bool {
        self.shared.running.load(Ordering::Relaxed)
    }

    pub fn tried(&self) -> usize {
        self.shared.tried.load(Ordering::Relaxed)
    }

    pub fn subnets(&self) -> Vec<String> {
        self.shared
            .subnets
            .lock()
            .map(|subnets| subnets.clone())
            .unwrap_or_default()
    }

    pub fn age(&self) -> Option<Duration> {
        let finished = self.shared.finished.lock().ok()?;
        finished.map(|when| when.elapsed())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_look_covers_the_subnet_and_never_tries_this_address() {
        let plan = ScanPlan {
            lan: true,
            ports: vec![7005],
            addresses: vec!["example.org".into(), "192.168.0.9:7000".into()],
        };
        let (near, far) = targets(&plan, &["192.168.0.4".parse().unwrap()]);
        assert!(near.contains(&"192.168.0.1:7000".to_string()));
        assert!(near.contains(&"192.168.0.1:7005".to_string()));
        assert!(!near.contains(&"192.168.0.4:7000".to_string()));
        assert!(!near.contains(&"192.168.0.9:7000".to_string()));
        assert!(far.contains(&"example.org:7000".to_string()));
    }

    #[test]
    fn with_the_lan_off_it_still_looks_on_this_computer() {
        let (near, _) = targets(&ScanPlan::default(), &["192.168.0.4".parse().unwrap()]);
        assert!(near.iter().all(|address| address.starts_with("127.0.0.1:")));
        assert!(near.contains(&"127.0.0.1:7100".to_string()));
    }
}
