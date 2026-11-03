//! Coin swaps: the orders a person gives, and the steps the two chains allow, a second at a time.
//! Each side pays with the coin its app runs. The coin it gets runs in a second node beside it,
//! on the same keys, for as long as a swap needs it.
use super::{bind_host, Service};
use crate::core::coins::{coin_folder, format_coins, known_coin, parse_coins, read_coin_file};
use crate::core::commands::SwapOrder;
use crate::core::host::Plan;
use crate::core::rows::{device_kind, hostname};
use crate::core::swap::{make_offer, take_offer, Accept, Happened, Role, Side};
use crate::core::swap_book::{coins_at_hand, notice_of};
use crate::core::swap_text::{find_coin, OfferText, MOST_ADDRESSES};
use crate::discovery::{ask, local_networks};
use crate::settings::PORT_TRIES;
use olc_core::CoinConfig;
use olc_node::Card;
use std::path::PathBuf;
use std::time::Duration;

/// How long the taker waits for each of the maker's addresses to answer.
const ASK_WAIT: Duration = Duration::from_secs(3);

type Said = Result<(&'static str, String), (&'static str, String)>;

const HELD: (&str, String) = ("notice.held", String::new());

impl Service {
    pub(super) fn swap_order(&mut self, order: SwapOrder) {
        let said = match order {
            SwapOrder::Offer { coin, give, take, wait_minutes } => self.offer_swap(&coin, (&give, &take), wait_minutes),
            SwapOrder::Accept(text) => self.accept_swap(&text),
            SwapOrder::Answer(text) => self.answer_swap(&text),
            SwapOrder::Cancel(id) => self.cancel_swap(&id),
            SwapOrder::List => {
                let (folder, height) = self.main_coin();
                Ok(("swap.list", self.swaps.lines((&folder, height))))
            }
        };
        match (said, self.swaps.save()) {
            (Ok((key, detail)), Ok(())) => self.say(key, detail, true),
            (Ok(_), Err(error)) => self.say("swap.not_saved", error, false),
            (Err((key, detail)), _) => self.say(key, detail, false),
        }
    }

    /// The folder name and height of the coin the main node runs.
    fn main_coin(&self) -> (String, u64) {
        let view = self.host.view();
        (view.coin.as_ref().map(coin_folder).unwrap_or_default(), view.height)
    }

    fn main_config(&self) -> Result<CoinConfig, (&'static str, String)> {
        let runner = self.host.ready().map_err(|_| HELD)?;
        Ok(runner.engine.chain().config().clone())
    }

    /// What the node on the other coin starts from: this node's way of reaching others.
    fn beside_plan(&self, coin: CoinConfig) -> Plan {
        Plan {
            coin,
            folder: PathBuf::new(),
            wallet_folder: self.paths.wallet(),
            bind_host: bind_host(&self.settings),
            port: self.settings.port,
            port_tries: PORT_TRIES,
            peers: self.every_address(),
            cap: 0,
            mining: false,
            card: Card { name: hostname(), device: device_kind(), build: env!("CARGO_PKG_VERSION").into(), ..Card::default() },
            storage: None,
            shares_wallet: true,
        }
    }

    /// Every address this app knows of. Peers are saved for the coin this app runs, so someone who
    /// moved to a coin of their own has none for the coin they swap with, and its node tries all.
    fn every_address(&self) -> Vec<String> {
        let mut all = self.settings.to_dial(Some(self.settings.port));
        for address in self.settings.book.iter().cloned().chain(self.found.reachable()) {
            if !all.contains(&address) {
                all.push(address);
            }
        }
        all
    }

    /// Where this app's main node listens, as another computer dials it: its address on each
    /// local network when it listens there, and this computer's own.
    fn my_addresses(&self) -> Vec<String> {
        let Some(port) = self.host.port else { return Vec::new() };
        let lan = if self.settings.listen_lan { local_networks() } else { Vec::new() };
        let hosts = lan.into_iter().map(|ip| ip.to_string()).chain(["127.0.0.1".to_string()]);
        hosts.map(|host| format!("{host}:{port}")).take(MOST_ADDRESSES).collect()
    }

    /// Lets the node on the other coin dial the other side's node.
    fn follow(&self, addresses: &[String]) {
        if let Some((_, other)) = &self.swaps.other {
            addresses.iter().for_each(|address| other.connect(address.clone()));
        }
    }

    /// Runs the node on `other`, unless an open swap already holds one on a third coin.
    fn run_other(&mut self, main: &CoinConfig, other: &CoinConfig) -> Result<(), (&'static str, String)> {
        let open = self.swaps.other_coin(&coin_folder(main));
        if let Some(open) = open.filter(|open| coin_folder(open) != coin_folder(other)) {
            return Err(("swap.busy", open.ticker));
        }
        let plan = self.beside_plan(main.clone());
        self.swaps.run_other(&self.paths, other, &plan).map_err(|error| ("swap.failed", error))
    }

    fn offer_swap(&mut self, coin: &str, (give, take): (&str, &str), wait_minutes: u32) -> Said {
        let units = |text: &str| parse_coins(text).filter(|units| *units > 0).ok_or(("notice.bad_amount", text.to_string()));
        let (give, take) = (units(give)?, units(take)?);
        let other = known_coin(&self.paths, coin.trim()).or_else(|| read_coin_file(coin.trim()).ok());
        let other = other.ok_or(("swap.no_coin", coin.to_string()))?;
        let main = self.main_config()?;
        if coin_folder(&other) == coin_folder(&main) {
            return Err(("swap.same_coin", other.ticker));
        }
        let fee = main.rules.min_fee;
        if self.host.view().spendable < give.saturating_add(fee) {
            return Err(("swap.not_enough", format!("{} {}", format_coins(give), main.ticker)));
        }
        self.run_other(&main, &other)?;
        let runner = self.host.ready().map_err(|_| HELD)?;
        let sides = (Side { coin: main, units: give }, Side { coin: other, units: take });
        let made = make_offer(&runner.engine, sides, wait_minutes, self.my_addresses());
        drop(runner);
        let swap = made.map_err(|error| ("swap.failed", error))?;
        let text = swap.text();
        self.swaps.list.push(swap);
        Ok(("swap.offered", text))
    }

    fn accept_swap(&mut self, text: &str) -> Said {
        let read = OfferText::from_text(text).ok_or(("swap.bad_text", String::new()))?;
        if self.swaps.list.iter().any(|swap| swap.id() == read.id()) {
            return Err(("swap.known", String::new()));
        }
        let main = self.main_config()?;
        // The taker pays with what the maker takes, and that is the coin this app runs.
        if !read.take.is(&main) {
            return Err(("swap.pay_with_main", read.take.ticker.clone()));
        }
        if read.give.is(&main) {
            return Err(("swap.same_coin", main.ticker));
        }
        if self.host.view().spendable < read.take.units.saturating_add(main.rules.min_fee) {
            return Err(("swap.not_enough", format!("{} {}", format_coins(read.take.units), main.ticker)));
        }
        // The text names the maker's coin by id. A coin this app ran serves; any other comes from
        // the maker's node, which runs it.
        let fetch = |address: &str| ask(address, ASK_WAIT).map(|hello| hello.coin);
        let give = find_coin(&read.give, coins_at_hand(&self.paths), &read.at, fetch);
        let give = give.ok_or(("swap.coin_not_found", read.give.ticker.clone()))?;
        let offer = read.with_coins(give, main.clone()).ok_or(("swap.bad_text", String::new()))?;
        self.run_other(&main, &offer.give.coin)?;
        let runner = self.host.ready().map_err(|_| HELD)?;
        let swap = take_offer(offer, &runner.engine, self.my_addresses());
        drop(runner);
        self.follow(&swap.offer.at);
        let text = swap.text();
        self.swaps.list.push(swap);
        Ok(("swap.accepted", text))
    }

    fn answer_swap(&mut self, text: &str) -> Said {
        let accept = Accept::from_text(text).ok_or(("swap.bad_text", String::new()))?;
        let mine = |swap: &&mut crate::core::swap::Swap| swap.role == Role::Maker && swap.id() == accept.offer;
        let swap = self.swaps.list.iter_mut().find(mine).ok_or(("swap.unknown", String::new()))?;
        if swap.accept.is_some() || swap.outcome.is_some() {
            return Err(("swap.known", String::new()));
        }
        let at = accept.at.clone();
        swap.accept = Some(accept);
        let (give, get) = swap.coins();
        let said = format!("{} {} > {} {}", format_coins(give.units), give.coin.ticker, format_coins(get.units), get.coin.ticker);
        self.follow(&at);
        Ok(("swap.started", said))
    }

    fn cancel_swap(&mut self, id: &str) -> Said {
        let swap = self.swaps.find(id).ok_or(("swap.unknown", id.to_string()))?;
        match swap.cancel() {
            true => Ok(("swap.cancelled", id.to_string())),
            false => Err(("swap.cannot_cancel", id.to_string())),
        }
    }

    /// Moves every open swap on, and says what happened.
    pub(super) fn step_swaps(&mut self) {
        let fresh = self.swaps.refresh_known(&self.paths);
        if !self.swaps.rested() {
            return;
        }
        let Ok(main) = self.main_config() else { return };
        let folder = coin_folder(&main);
        match self.swaps.other_coin(&folder) {
            Some(other) if self.run_other(&main, &other).is_ok() => {}
            Some(_) => return,
            None => return self.swaps.let_other_go(&folder),
        }
        if fresh {
            self.follow(&self.every_address());
            self.follow(&self.swaps.their_addresses());
        }
        let mut heard = Vec::new();
        {
            let swaps = &mut self.swaps;
            let Some((_, other)) = &swaps.other else { return };
            let (Ok(mut here), Ok(mut there)) = (self.host.ready(), other.ready()) else { return };
            for swap in swaps.list.iter_mut().filter(|swap| swap.outcome.is_none()) {
                let pays_here = coin_folder(&swap.coins().0.coin) == folder;
                let (mine, theirs) = match pays_here {
                    true => (&mut here.engine, &mut there.engine),
                    false => (&mut there.engine, &mut here.engine),
                };
                if let Some(happened) = swap.step(mine, theirs) {
                    let (give, get) = swap.coins();
                    let side = match happened {
                        Happened::Claimed | Happened::Swapped => get,
                        _ => give,
                    };
                    heard.push((happened, format!("{} {}", format_coins(side.units), side.coin.ticker)));
                }
            }
        }
        if heard.is_empty() {
            return;
        }
        if let Err(error) = self.swaps.save() {
            self.say("swap.not_saved", error, false);
        }
        for (happened, detail) in heard {
            self.say(notice_of(happened), detail, happened != Happened::TooLate);
        }
    }
}
