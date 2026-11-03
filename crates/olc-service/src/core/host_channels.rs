//! Payment channels in the desktop node. The channel node lives beside the engine, reads blocks
//! and the pool through it, and talks to other nodes on the network link only, so a board never
//! hears a channel message. Its files sit in the coin's folder: each coin has its own channels.
use super::coins::{format_coins, parse_coins};
use super::commands::ChannelOrder;
use super::host::{Host, State};
use super::host_parts::unix_ms;
use super::host_peers::{peer_listing, peer_order, peers_of, reach_at, PeerOrder, PeersView};
use olc_link_tcp::TcpControl;
use olc_channel::invoice::{coin_tag, Invoice, InvoiceError};
use olc_channel::{ChannelNode, Ground, NodeId, Out, Quote, Refusal, Said};
use olc_core::{Address, Hash, OutPoint, Tx, TxOut};
use olc_node::{Engine, PeerId, Runner};
use std::collections::{BTreeMap, VecDeque};
use olc_platform::platform;
use std::path::{Path, PathBuf};

/// Channel files hold the keys to the channel's coins, so only this computer's user reads them.
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    platform().write_private(path, bytes)
}

pub use olc_channel::{ChannelView, InvoiceView, PaymentView};
pub use olc_channel::{PayState, Phase};

/// The last request read before paying it, numbered so a page tells a new answer from an old one:
/// who it pays and at what fee, or the words and detail of why it cannot be paid.
pub type Quoted = (u64, String, Result<Quote, (&'static str, String)>);

mod paying;
mod reach;
pub use reach::read_channel_address;
use reach::Waiting;

/// The network link, the only one channel messages travel on.
const TCP_LINK: usize = 0;
const TICK_EVERY_MS: u64 = 250;
const MOST_EVENTS: usize = 20;
const MOST_ROUTES: usize = 256;
/// The lowest fee a channel transaction pays, whatever the coin's own floor.
const LOWEST_FEE: u64 = 1_000;

/// Something that happened to a channel, for the channels page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChannelEvent {
    pub number: u64,
    pub key: &'static str,
    pub detail: String,
}

/// The channels as the screen shows them.
#[derive(Clone, Debug, Default)]
pub struct ChannelsView {
    /// False on a node that holds headers only, and before a chain is open.
    pub running: bool,
    /// This node's key in hex, which others name to open a channel to it.
    pub key: String,
    pub channels: Vec<ChannelView>,
    pub payments: Vec<PaymentView>,
    pub invoices: Vec<InvoiceView>,
    /// Nodes heard lately that can hold a channel: key in hex, and name.
    pub peers: Vec<(String, String)>,
    /// Newest first.
    pub events: Vec<ChannelEvent>,
    pub quote: Option<Quoted>,
    /// What another person pastes to open a channel with this node: `key@host:port`.
    pub address: String,
    /// Addresses dialled for a channel that opens once the node there answers.
    pub reaching: Vec<String>,
    /// True when only programs on this computer can reach the address.
    pub only_here: bool,
}

#[derive(Default)]
pub struct ChannelDesk {
    node: Option<ChannelNode>,
    folder: Option<PathBuf>,
    /// Who on the network link each channel node was last heard from.
    routes: BTreeMap<NodeId, PeerId>,
    ticked_ms: u64,
    view: ChannelsView,
    events: VecDeque<ChannelEvent>,
    number: u64,
    quote: Option<Quoted>,
    waiting: Vec<Waiting>,
    /// The last event before the payment sent last, until its ending is told.
    paying_from: Option<u64>,
}

/// The chain, wallet and pool of the running node, as the channel node asks for them.
struct NodeGround<'a> {
    engine: &'a mut Engine,
}

fn fee_of(engine: &Engine) -> u64 {
    engine.rules().min_fee.max(LOWEST_FEE)
}

impl Ground for NodeGround<'_> {
    fn height(&self) -> u64 {
        self.engine.chain().height()
    }

    /// Read from the log, so a watcher that fell behind the recent window still finds it.
    fn block_at(&self, height: u64) -> Option<(Hash, Vec<Tx>)> {
        let block = self.engine.chain().blocks_from(height, 1).pop()?;
        Some((block.id(), block.txs))
    }

    fn coin(&self, point: &OutPoint) -> Option<(TxOut, u64)> {
        self.engine.chain().coin_at(point).map(|coin| (coin.output.clone(), coin.height))
    }

    fn coin_id(&self) -> Option<Hash> {
        let chain = self.engine.chain();
        chain.signing_id(chain.height() + 1)
    }

    fn genesis(&self) -> Hash {
        self.engine.chain().genesis_id()
    }

    fn fee(&self) -> u64 {
        fee_of(self.engine)
    }

    fn pay_to(&self) -> Address {
        self.engine.address()
    }

    fn pick_funding(&mut self, amount: u64) -> Result<Vec<OutPoint>, String> {
        let fee = fee_of(self.engine);
        self.engine.pick_funding(amount, fee).map_err(|error| format!("{error:?}"))
    }

    fn fund(&mut self, lock: &[u8], amount: u64, from: &[OutPoint]) -> Result<Tx, String> {
        let fee = fee_of(self.engine);
        self.engine.sign_funding(lock, amount, fee, from).map_err(|error| format!("{error:?}"))
    }

    fn publish(&mut self, tx: &Tx) -> Result<(), String> {
        self.engine.publish(tx.clone()).map(|_| ()).map_err(|error| format!("{error:?}"))
    }

    fn waiting(&self, id: &Hash) -> bool {
        self.engine.in_pool(id)
    }

    fn now_s(&self) -> u64 {
        unix_ms() / 1_000
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// The start of a channel's id, as people name it.
pub fn short_id(id: &[u8]) -> String {
    hex(&id[..id.len().min(4)])
}

/// The words for a refusal.
pub fn refusal_key(refusal: Refusal) -> &'static str {
    match refusal {
        Refusal::ThisNode => "ch.no_this_node",
        Refusal::NoV2Yet => "ch.no_v2",
        Refusal::TooLittle => "ch.no_too_little",
        Refusal::NotSaved => "ch.no_saved",
        Refusal::NoSuchChannel => "ch.no_channel",
        Refusal::Busy => "ch.no_busy",
        Refusal::NotOpen => "ch.no_open",
        Refusal::NotARequest => "ch.no_request",
        Refusal::BadSignature => "ch.no_signature",
        Refusal::OtherCoin => "ch.no_other_coin",
        Refusal::Expired => "ch.no_expired",
        Refusal::OwnRequest => "ch.no_own",
        Refusal::AlreadyPaid => "ch.no_paid",
        Refusal::NoRoute => "ch.no_route",
        Refusal::NoCoins => "notice.not_enough",
    }
}

/// The height from which the coin takes version 2 transactions, and so channels.
fn channels_from(runner: &Runner) -> Option<u64> {
    let config = runner.engine.chain().config();
    let forked = config.forks.iter().map(|fork| fork.from_height).min();
    let from = forked.map_or(config.upgrade_from, |height| height.min(config.upgrade_from));
    (from != u64::MAX).then_some(from)
}

fn event_of(said: Said, ticker: &str) -> (&'static str, String) {
    let coins = |amount: u64| format!("{} {ticker}", format_coins(amount));
    match said {
        Said::Opened(id) => ("ch.said_opened", short_id(&id)),
        Said::Refused(why) => ("ch.said_refused", why.chars().take(80).collect()),
        Said::Paid { amount, .. } => ("ch.said_paid", coins(amount)),
        Said::PayFailed { hash } => ("ch.said_pay_failed", short_id(&hash)),
        Said::Received { amount, .. } => ("ch.said_received", coins(amount)),
        Said::Closed(id) => ("ch.said_closed", short_id(&id)),
        Said::Penalty { txid, .. } => ("ch.said_penalty", hex(&txid)),
    }
}

/// One turn of the channel node, inside the node's own turn.
pub(super) fn pump(state: &State, runner: &mut Runner) {
    let folder = state.folder.lock().ok().and_then(|folder| folder.clone());
    if let Ok(mut desk) = state.channels.lock() {
        desk.turn(runner, folder, &state.tcp);
    }
}

/// Every connection of the node now, named by the channel node heard on it where one was.
pub(super) fn peers_now(state: &State, runner: &Runner) -> PeersView {
    let names = state.channels.lock().map(|desk| desk.names(runner)).unwrap_or_default();
    peers_of(&state.tcp, runner, &|number| names.get(&number).cloned())
}

/// What the screen shows of the channels now.
pub(super) fn view_of(state: &State) -> ChannelsView {
    state.channels.lock().map(|desk| desk.view.clone()).unwrap_or_default()
}

impl ChannelDesk {
    fn reopen(&mut self, folder: Option<PathBuf>, name: &str) {
        self.node = folder.as_ref().and_then(|folder| match ChannelNode::open(&folder.join("channels"), name, write_private) {
            Ok(node) => Some(node),
            Err(error) => {
                eprintln!("channels: {error}");
                None
            }
        });
        self.folder = folder;
        self.routes.clear();
        self.events.clear();
        self.view = ChannelsView::default();
    }

    fn turn(&mut self, runner: &mut Runner, folder: Option<PathBuf>, tcp: &TcpControl) {
        // A node that holds headers only cannot watch the chain for a cheat.
        let folder = folder.filter(|_| runner.engine.light().is_none());
        if folder != self.folder {
            let name = runner.card.name.clone();
            self.reopen(folder, &name);
        }
        runner.channels = self.node.is_some();
        let Some(node) = self.node.as_mut() else {
            while runner.take_channel().is_some() {}
            return;
        };
        // Before any block can connect this turn: the blocks the watcher has yet to read stay,
        // and coins a channel will be funded from stay out of every other payment.
        runner.engine.chain_mut().set_hold(node.keep_blocks_from());
        runner.engine.hold_coins(node.held_coins());
        while let Some((link, peer, bytes)) = runner.take_channel() {
            if link != TCP_LINK {
                continue;
            }
            let mut ground = NodeGround { engine: &mut runner.engine };
            if let Some(from) = node.receive(&bytes, &mut ground) {
                if self.routes.len() >= MOST_ROUTES && !self.routes.contains_key(&from) {
                    self.routes.pop_first();
                }
                self.routes.insert(from, peer);
            }
        }
        let now = unix_ms();
        self.open_reached(runner, now);
        let Some(node) = self.node.as_mut() else {
            return;
        };
        if now.saturating_sub(self.ticked_ms) >= TICK_EVERY_MS {
            self.ticked_ms = now;
            node.tick(now, &mut NodeGround { engine: &mut runner.engine });
            runner.engine.chain_mut().set_hold(node.keep_blocks_from());
            runner.engine.hold_coins(node.held_coins());
            self.refresh(runner, tcp);
        }
        self.send(runner);
    }

    fn tell(&mut self, key: &'static str, detail: String) {
        self.number += 1;
        if self.events.len() >= MOST_EVENTS {
            self.events.pop_back();
        }
        self.events.push_front(ChannelEvent { number: self.number, key, detail });
    }

    /// Sends what the channel node has to say, to the node it is for where that one is known.
    fn send(&mut self, runner: &mut Runner) {
        let Some(node) = self.node.as_mut() else {
            return;
        };
        while let Some(out) = node.take_out() {
            match out {
                Out::To(to, bytes) => runner.send_channel(TCP_LINK, self.routes.get(&to).copied(), bytes),
                Out::All { bytes, .. } => runner.send_channel(TCP_LINK, None, bytes),
            }
        }
    }

    fn refresh(&mut self, runner: &Runner, tcp: &TcpControl) {
        let Some(node) = self.node.as_mut() else {
            return;
        };
        let ticker = runner.engine.chain().config().ticker.clone();
        let mut heard = Vec::new();
        while let Some(said) = node.take_said() {
            heard.push(event_of(said, &ticker));
        }
        for (key, detail) in heard {
            self.tell(key, detail);
        }
        let Some(node) = self.node.as_ref() else {
            return;
        };
        let height = runner.engine.chain().height();
        let (at, only_here) = reach_at(tcp);
        let key = hex(&node.node_key().0);
        self.view = ChannelsView {
            address: at.map(|at| format!("{key}@{at}")).unwrap_or_default(),
            reaching: self.waiting.iter().map(|waiting| waiting.address.clone()).collect(),
            only_here,
            running: true,
            key,
            channels: node.views(height),
            payments: node.payments(),
            invoices: node.invoices(unix_ms() / 1_000),
            peers: node.peers().into_iter().map(|(node, name)| (hex(&node.0), name)).collect(),
            events: self.events.iter().cloned().collect(),
            quote: self.quote.clone(),
        };
    }

    /// The channel node heard on each connection, by the connection's number. A route names a
    /// neighbour, which may be heard on several connections of the network link.
    fn names(&self, runner: &Runner) -> BTreeMap<u32, (String, String)> {
        let Some(node) = self.node.as_ref() else {
            return BTreeMap::new();
        };
        let routes = &self.routes;
        let neighbours = runner.neighbours();
        node.peers()
            .into_iter()
            .filter_map(|(key, name)| Some((neighbours.iter().find(|one| Some(&one.peer) == routes.get(&key))?, name, key)))
            .flat_map(|(one, name, key)| {
                let numbers = one.paths.iter().filter(|path| path.link == TCP_LINK).map(|path| path.number);
                numbers.map(move |number| (number, (name.clone(), hex(&key.0)))).collect::<Vec<_>>()
            })
            .collect()
    }

    fn carry(&mut self, runner: &mut Runner, tcp: &TcpControl, order: ChannelOrder) -> (&'static str, String, bool) {
        // These reach the network link as well as the channel node, so they come first.
        let order = match order {
            ChannelOrder::Open { peer, amount, delay, push } => return self.open_order(runner, &peer, &amount, &push, &delay),
            ChannelOrder::Peers => {
                let names = self.names(runner);
                return ("ch.list", peer_listing(&peers_of(tcp, runner, &|number| names.get(&number).cloned())), true);
            }
            ChannelOrder::Disconnect(number) => return peer_order(tcp, PeerOrder::Disconnect(number)),
            ChannelOrder::Ban { address, hours } => return peer_order(tcp, PeerOrder::Ban { address, hours }),
            ChannelOrder::Unban(address) => return peer_order(tcp, PeerOrder::Unban(address)),
            order => order,
        };
        let Some(node) = self.node.as_mut() else {
            return ("ch.no_running", String::new(), false);
        };
        let v2_from = channels_from(runner);
        let mut ground = NodeGround { engine: &mut runner.engine };
        let refused = |refusal: Refusal| match refusal {
            Refusal::NoV2Yet => match v2_from {
                Some(height) => ("ch.no_v2", height.to_string(), false),
                None => ("ch.no_v2_never", String::new(), false),
            },
            other => (refusal_key(other), String::new(), false),
        };
        match order {
            ChannelOrder::Quote { text } => {
                let said = node.quote(text.trim(), &ground).map_err(|refusal| {
                    let (key, detail, _) = refused(refusal);
                    (key, detail)
                });
                self.number += 1;
                self.quote = Some((self.number, text, said));
                ("", String::new(), true)
            }
            ChannelOrder::Show => ("ch.list", listing(&self.view), true),
            ChannelOrder::Open { .. }
            | ChannelOrder::Peers
            | ChannelOrder::Disconnect(_)
            | ChannelOrder::Ban { .. }
            | ChannelOrder::Unban(_) => {
                ("", String::new(), false)
            }
            ChannelOrder::Close { channel, force } => {
                let Some(id) = node.find(&channel) else {
                    return refused(Refusal::NoSuchChannel);
                };
                match node.close(&id, force, &mut ground) {
                    Ok(()) if force => ("ch.closing_alone", short_id(&id), true),
                    Ok(()) => ("ch.closing", short_id(&id), true),
                    Err(refusal) => refused(refusal),
                }
            }
            ChannelOrder::Invoice { amount, note } => {
                let Some(units) = parse_coins(amount.trim()).filter(|units| *units > 0) else {
                    return ("notice.bad_amount", String::new(), false);
                };
                match node.make_invoice(units, note.trim(), &mut ground) {
                    Ok(text) => ("ch.request_made", text, true),
                    Err(refusal) => refused(refusal),
                }
            }
            ChannelOrder::Pay { text } => {
                let amount = match node.read_invoice(&text, &ground) {
                    Ok(invoice) => invoice.amount,
                    Err(refusal) => return refused(refusal),
                };
                let ticker = ground.engine.chain().config().ticker.clone();
                let said = match node.pay(&text, &mut ground) {
                    Ok(_) => ("ch.paying", format!("{} {ticker}", format_coins(amount)), true),
                    Err(refusal) => return refused(refusal),
                };
                self.paying_now();
                said
            }
        }
    }
}

impl Host {
    /// Carries out a channel order on the running node and says how it went.
    pub fn carry_channel(&self, order: ChannelOrder) -> (&'static str, String, bool) {
        let Ok(mut runner) = self.ready() else {
            return ("ch.no_running", String::new(), false);
        };
        let Ok(mut desk) = self.state.channels.lock() else {
            return ("ch.no_running", String::new(), false);
        };
        let said = desk.carry(&mut runner, &self.state.tcp, order);
        desk.refresh(&runner, &self.state.tcp);
        desk.send(&mut runner);
        said
    }

    pub fn channels(&self) -> ChannelsView {
        view_of(&self.state)
    }
}

/// What a payment request asks, read before paying it: the amount and the note, or the key of
/// why it cannot be paid.
pub fn read_request(text: &str, genesis: &Hash) -> Result<(u64, String), &'static str> {
    match Invoice::read(text.trim(), &coin_tag(genesis), unix_ms() / 1_000) {
        Ok(invoice) => Ok((invoice.amount, invoice.note)),
        Err(InvoiceError::NotOne) => Err("ch.no_request"),
        Err(InvoiceError::BadSignature) => Err("ch.no_signature"),
        Err(InvoiceError::OtherCoin) => Err("ch.no_other_coin"),
        Err(InvoiceError::Expired) => Err("ch.no_expired"),
    }
}

/// How a channel stands, in a few words for the command line.
pub fn standing(channel: &ChannelView) -> String {
    match channel.phase {
        Phase::Asked => "asked, waiting for an answer".into(),
        Phase::Accepted | Phase::FundingSent => "agreeing on the first state".into(),
        Phase::Confirming => "waiting for blocks".into(),
        Phase::Open => "open".into(),
        Phase::Closing => "closing together".into(),
        Phase::ForceClosing if channel.cheated => "an old state was sent, so every coin is being taken".into(),
        Phase::ForceClosing => match channel.blocks_left {
            Some(blocks) if blocks > 0 => format!("closed alone, coins back in {blocks} blocks"),
            _ => "closed alone, coins coming back".into(),
        },
        Phase::Closed if channel.cheated => "closed, every coin taken for the old state".into(),
        Phase::Closed => "closed".into(),
        Phase::Failed => "did not open".into(),
    }
}

/// Everything about the channels, as `olc channels` prints it.
fn listing(view: &ChannelsView) -> String {
    let mut lines = vec![format!("This node: {}", view.key)];
    if !view.address.is_empty() {
        lines.push(format!("Channel address: {}", view.address));
    }
    lines.extend(view.reaching.iter().map(|address| format!("Connecting to {address}")));
    for (key, name) in &view.peers {
        lines.push(format!("Nearby: {} {name}", &key[..key.len().min(16)]));
    }
    for channel in &view.channels {
        let name = if channel.peer_name.is_empty() { hex(&channel.peer.0[..8]) } else { channel.peer_name.clone() };
        lines.push(format!(
            "Channel {} with {name}: {}. Yours {}, theirs {}.",
            short_id(&channel.id),
            standing(channel),
            format_coins(channel.ours),
            format_coins(channel.theirs),
        ));
        lines.push(format!("  opened by {}:{}", hex(&channel.funding.txid), channel.funding.index));
        if let Some(close) = channel.closed_by {
            lines.push(format!("  closed by {}", hex(&close)));
        }
        if let Some(penalty) = channel.penalty {
            lines.push(format!("  penalty {}", hex(&penalty)));
        }
        lines.extend(channel.claims.iter().map(|claim| format!("  taken back by {}", hex(claim))));
    }
    for payment in &view.payments {
        let state = match payment.state {
            PayState::Pending => "on its way",
            PayState::Paid => "paid",
            PayState::Failed => "failed",
        };
        let fee = format_coins(payment.fee);
        lines.push(format!("Payment {}: {} {state}, fee {fee}", short_id(&payment.hash), format_coins(payment.amount)));
    }
    for invoice in view.invoices.iter().take(5) {
        let state = if invoice.paid { "paid" } else if invoice.expired { "expired" } else { "waiting" };
        lines.push(format!("Request {} {state}: {}", format_coins(invoice.amount), invoice.text));
    }
    lines.join("\n")
}
