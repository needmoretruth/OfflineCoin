//! What a person chose about pools: the one this app runs, and the pools each miner here joins.
//! It is kept with the settings, so a restart finds the same choices.
use olc_control::BoardId;
use olc_pool::{Payout, Plan, MOST_FEE_BPS, MOST_LAST_SHARES, PORT};
use serde::{Deserialize, Serialize};

/// A miner on this computer, or a board it reaches.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Who {
    /// The app's own miner.
    Own,
    Node(u32),
    Board(BoardId),
}

/// How a block's coins are shared out, as a person picks it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Method {
    #[default]
    Proportional,
    LastShares,
}

/// The shares PPLNS looks back over when nobody chose: about an hour and a half of one worker.
pub const LAST_SHARES: u64 = 1_000;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PoolSettings {
    pub port: u16,
    /// Other computers may join: the port listens on every network, not only this computer.
    pub open: bool,
    pub method: Method,
    pub last_shares: u64,
    /// Hundredths of a percent the pool keeps, at most 5 %.
    pub fee_bps: u32,
    /// One share difficulty for every worker. 0 lets each worker's own move.
    pub fixed_difficulty: u64,
    /// The miners here that work in the pool this app runs, when it runs.
    pub workers: Vec<Who>,
    /// Pools elsewhere the miners here joined.
    pub joins: Vec<Join>,
    /// The pool ran when the app closed, so it runs again when the app opens.
    pub run_on_start: bool,
    /// Listens on 127.0.0.1 even when open, for tests that play both computers on one. Never
    /// saved, so no person's pool is ever held to this computer by it.
    #[serde(skip)]
    pub bind_loopback: bool,
}

impl Default for PoolSettings {
    fn default() -> Self {
        Self {
            port: PORT,
            open: false,
            method: Method::Proportional,
            last_shares: LAST_SHARES,
            fee_bps: 0,
            fixed_difficulty: 0,
            workers: vec![Who::Own],
            joins: Vec::new(),
            run_on_start: false,
            bind_loopback: false,
        }
    }
}

impl PoolSettings {
    pub fn plan(&self) -> Plan {
        let payout = match self.method {
            Method::Proportional => Payout::Proportional,
            Method::LastShares => Payout::LastShares { shares: self.last_shares.clamp(1, MOST_LAST_SHARES) },
        };
        Plan {
            payout,
            fee_bps: self.fee_bps.min(MOST_FEE_BPS),
            fixed_difficulty: (self.fixed_difficulty > 0).then_some(self.fixed_difficulty),
        }
    }
}

/// A miner here working in a pool somewhere: `host:port`, and the name it signs in with,
/// `address.worker`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Join {
    pub who: Who,
    pub address: String,
    pub user: String,
}

/// What a person does with pools.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PoolOrder {
    /// Starts the pool here with these choices.
    Start(PoolSettings),
    Stop,
    /// Puts a miner here to work in the pool at `address` as `user`.
    Join(Join),
    /// Takes a miner here out of the pool it joined.
    Leave(Who),
    /// Says how the pools stand, for the command line.
    Show,
}

/// A worker's name as Stratum takes it: no spaces, nothing a line could break on, not too long.
pub fn worker_part(name: &str) -> String {
    let kept: String = name
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
        .take(32)
        .collect();
    let kept = kept.trim_matches('-').to_string();
    if kept.is_empty() {
        "miner".into()
    } else {
        kept
    }
}

/// This computer's name as the system knows it, fit for a worker's name: what a person sees in
/// the pool's list when this computer joins one.
pub fn computer_name() -> String {
    let named = std::env::var("COMPUTERNAME").or_else(|_| std::env::var("HOSTNAME")).ok();
    let named = named.or_else(|| std::fs::read_to_string("/etc/hostname").ok());
    worker_part(named.as_deref().unwrap_or_default().trim().split('.').next().unwrap_or_default())
}

/// `host:port`, with the pool's usual port when none is given.
pub fn pool_address(text: &str) -> Option<String> {
    let text = text.trim().trim_start_matches("stratum+tcp://").trim_end_matches('/');
    if text.is_empty() || text.chars().any(char::is_whitespace) {
        return None;
    }
    match text.rsplit_once(':') {
        Some((host, port)) if !host.is_empty() && port.parse::<u16>().is_ok_and(|port| port > 0) => Some(text.into()),
        Some(_) => None,
        None => Some(format!("{text}:{PORT}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pool_address_takes_the_forms_miners_write() {
        assert_eq!(pool_address("192.168.0.5"), Some("192.168.0.5:3333".into()));
        assert_eq!(pool_address(" stratum+tcp://pool.example:4444 "), Some("pool.example:4444".into()));
        assert_eq!(pool_address("host:0"), None);
        assert_eq!(pool_address("host:port"), None);
        assert_eq!(pool_address("two words"), None);
        assert_eq!(worker_part("Kitchen PC #2"), "Kitchen-PC--2");
        assert_eq!(worker_part("  "), "miner");
    }

    #[test]
    fn the_app_is_the_first_worker_of_a_new_pool() {
        let settings = PoolSettings::default();
        assert_eq!(settings.workers, vec![Who::Own]);
        assert_eq!(settings.port, 3333);
        assert!(!settings.open, "only this computer until a person opens it");
        assert_eq!(settings.plan().payout, Payout::Proportional);
    }
}
