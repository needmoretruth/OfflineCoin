//! The swaps this wallet takes part in, kept beside its keys so a restart picks each one up where
//! it stood, and the node this app runs on the other coin while one is open.
use super::coins::{coin_folder, format_coins, known_coin, save_coin};
use super::host::{Host, Plan};
use super::snapshot::{SwapView, SwapsView};
use super::swap::{Happened, Role, Stage, Swap};
use crate::settings::Paths;
use olc_core::CoinConfig;
use olc_store::replace_file;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

const FILE: &str = "swaps.json";
/// How often the swaps look at the two chains.
const STEP_EVERY: Duration = Duration::from_secs(1);
/// How often the list of coins this app ran is read again.
const KNOWN_EVERY: Duration = Duration::from_secs(5);

#[derive(Default)]
pub struct Swaps {
    pub list: Vec<Swap>,
    /// The node on the other coin, and that coin's folder name.
    pub other: Option<(String, Host)>,
    file: PathBuf,
    stepped: Option<Instant>,
    /// Coins this app ran, by folder name and ticker, and when that was last read.
    known: Vec<(String, String)>,
    known_at: Option<Instant>,
}

impl Swaps {
    pub fn load(wallet_folder: PathBuf) -> Self {
        let file = wallet_folder.join(FILE);
        let read = fs::read(&file).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok());
        Self { list: read.unwrap_or_default(), file, ..Self::default() }
    }

    pub fn save(&self) -> Result<(), String> {
        let bytes = serde_json::to_vec_pretty(&self.list).map_err(|error| error.to_string())?;
        replace_file(&self.file, &bytes).map_err(|error| error.to_string())
    }

    pub fn open(&self) -> impl Iterator<Item = &Swap> {
        self.list.iter().filter(|swap| swap.outcome.is_none())
    }

    /// The coin besides `main` an open swap needs, if one does.
    pub fn other_coin(&self, main: &str) -> Option<CoinConfig> {
        let coins = self.open().flat_map(|swap| [&swap.offer.give.coin, &swap.offer.take.coin]);
        coins.into_iter().find(|coin| coin_folder(coin) != main).cloned()
    }

    /// Whether a second look this soon would find anything new.
    pub fn rested(&mut self) -> bool {
        let rested = self.stepped.is_none_or(|at| at.elapsed() >= STEP_EVERY);
        if rested {
            self.stepped = Some(Instant::now());
        }
        rested
    }

    /// Reads again, now and then, which coins this app ran before.
    /// Reads the coins this app ran again, now and then. True when it did.
    pub fn refresh_known(&mut self, paths: &Paths) -> bool {
        if self.known_at.is_some_and(|at| at.elapsed() < KNOWN_EVERY) {
            return false;
        }
        self.known_at = Some(Instant::now());
        let folders = fs::read_dir(paths.root.join("coins")).into_iter().flatten().flatten();
        let names = folders.filter_map(|entry| entry.file_name().into_string().ok());
        let mut known: Vec<_> = names.filter_map(|name| Some((known_coin(paths, &name)?.ticker, name))).collect();
        known.sort();
        self.known = known.into_iter().map(|(ticker, name)| (name, ticker)).collect();
        true
    }

    /// Runs a node on `coin` beside the main one, or keeps the one that runs on it.
    pub fn run_other(&mut self, paths: &Paths, coin: &CoinConfig, beside: &Plan) -> Result<(), String> {
        let folder = coin_folder(coin);
        if self.other.as_ref().is_some_and(|(running, _)| *running == folder) {
            return Ok(());
        }
        self.other = None;
        // Kept beside its chain, the coin shows among the coins to swap for next time.
        save_coin(paths, coin)?;
        let plan = Plan {
            coin: coin.clone(),
            folder: paths.coin(&folder),
            wallet_folder: beside.wallet_folder.clone(),
            bind_host: beside.bind_host.clone(),
            port: beside.port.saturating_add(1),
            port_tries: beside.port_tries,
            peers: beside.peers.clone(),
            cap: 0,
            mining: false,
            card: beside.card.clone(),
            storage: None,
            shares_wallet: true,
        };
        self.other = Some((folder, Host::start(plan, None)?));
        Ok(())
    }

    /// Lets the node on the other coin go once no open swap needs it.
    pub fn let_other_go(&mut self, main: &str) {
        if self.other_coin(main).is_none() {
            self.other = None;
        }
    }

    /// Finds the swap whose id starts with `start`.
    pub fn find(&mut self, start: &str) -> Option<&mut Swap> {
        let start = start.trim().to_lowercase();
        let found = self.list.iter_mut().filter(|swap| hex(&swap.id()).starts_with(&start));
        let mut found: Vec<_> = found.collect();
        (found.len() == 1 && !start.is_empty()).then(|| found.remove(0))
    }

    pub fn view(&self, main: (&str, u64)) -> SwapsView {
        let other = self.other.as_ref().map(|(_, host)| host.view());
        let other_height = other.as_ref().map_or(0, |view| view.height);
        let height_on = |coin: &CoinConfig| if coin_folder(coin) == main.0 { main.1 } else { other_height };
        let swaps = self.list.iter().rev().map(|swap| {
            let (give, get) = swap.coins();
            SwapView {
                id: hex(&swap.id())[..8].to_string(),
                maker: swap.role == Role::Maker,
                stage: swap.stage(),
                give: (give.coin.ticker.clone(), give.units),
                get: (get.coin.ticker.clone(), get.units),
                text: matches!(swap.stage(), Stage::WaitingForAnswer | Stage::WaitingForTheirLock).then(|| swap.text()),
                refund_at: swap.my_refund_at(),
                height: height_on(&give.coin),
                wait_minutes: swap.offer.wait_minutes,
            }
        });
        let other = other.and_then(|view| Some((view.coin?.ticker, view.height, view.peers)));
        let coins = self.known.iter().filter(|(name, _)| name != main.0).cloned().collect();
        SwapsView { swaps: swaps.collect(), other, coins }
    }

    /// One line a person reads for each swap, newest first.
    pub fn lines(&self, main: (&str, u64)) -> String {
        let view = self.view(main);
        let line = |swap: &SwapView| {
            let (give, get) = (format_coins(swap.give.1), format_coins(swap.get.1));
            format!("{}  {give} {} for {get} {}  {}", swap.id, swap.give.0, swap.get.0, stage_words(swap.stage))
        };
        let lines: Vec<String> = view.swaps.iter().map(line).collect();
        if lines.is_empty() { "No swaps yet.".into() } else { lines.join("\n") }
    }
}

/// Where a swap stands, as the command line says it.
pub fn stage_words(stage: Stage) -> &'static str {
    use super::swap::Outcome;
    match stage {
        Stage::WaitingForAnswer => "waiting for their answer",
        Stage::WaitingForTheirLock => "waiting for their coins",
        Stage::Locked => "your coins are locked",
        Stage::Claiming => "taking their coins",
        Stage::Refunding => "taking your coins back",
        Stage::Ended(Outcome::Swapped) => "done",
        Stage::Ended(Outcome::Refunded) => "your coins came back",
        Stage::Ended(Outcome::Cancelled) => "cancelled",
        Stage::Ended(Outcome::TooLate) => "their coins came too late",
    }
}

/// The notice for what a step did.
pub fn notice_of(happened: Happened) -> &'static str {
    match happened {
        Happened::Locked => "swap.locked",
        Happened::Claimed => "swap.claimed",
        Happened::Refunded => "swap.refunding",
        Happened::Swapped => "swap.done",
        Happened::CameBack => "swap.came_back",
        Happened::TooLate => "swap.too_late",
    }
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
