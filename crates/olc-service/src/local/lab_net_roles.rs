//! Each participant's part beside being a node: miners fund those who pay, senders pay receivers,
//! spammers flood, miners work in the pool they were put in, attackers attack once they can, the
//! bug exploiter tries its tricks, and a node left with no peer is connected again.
use super::*;
use crate::local::lab_side::worker_name;
use olc_core::amount::format_coins;

/// A payment a sender makes: one coin, at a fee rate above the spammer's, so it goes first.
const SEND_AMOUNT: u64 = COIN;
const SEND_RATE: u64 = 5;
/// The attacker pays the receiver this many coins, then takes them back.
pub const ATTACK_AMOUNT: u64 = 10;
/// A participant asked for coins waits this long before asking again.
const FUNDING_WAIT: Duration = Duration::from_secs(20);
const HEAL_EVERY: Duration = Duration::from_secs(5);

impl Keeper {
    pub(super) fn roles(&mut self) {
        if self.supervisor.is_none() {
            return;
        }
        for at in 0..self.members.len() {
            let member = self.members[at].clone();
            let waits = match member.kind {
                _ if member.status.is_none() => Some("net.wait_start"),
                Kind::Miner => {
                    self.join_pool(&member);
                    None
                }
                Kind::Sender => self.funded(&member).or_else(|| self.send_payment(&member)),
                Kind::Spammer => self.funded(&member).or_else(|| self.spam(&member)),
                Kind::Exploiter => self.funded(&member).or_else(|| self.tricks(&member)),
                Kind::Attacker => self.attack_when_ready(&member),
                Kind::Pool => self.members.iter().all(|other| other.pool != Some(member.id)).then_some("net.wait_pool"),
                _ => None,
            };
            if let Some(member) = self.member_mut(member.id) {
                member.waits = waits;
            }
        }
        self.split_when_ready();
        self.heal();
    }

    fn tell(&mut self, id: u32, topic: &'static str, line: String) {
        if self.told.get(&(id, topic)) != Some(&line) {
            self.send(id, &line);
            self.told.insert((id, topic), line);
        }
    }

    /// Coins to pay with: until it holds half of what it needs, a miner with enough pays it.
    fn funded(&mut self, member: &Member) -> Option<&'static str> {
        let needs = member.kind.needs_coins();
        let holds = member.status.as_ref().map_or(0, |status| status.balance);
        if holds >= needs / 2 {
            return None;
        }
        let Some(address) = member.address().map(str::to_string) else {
            return Some("net.wait_start");
        };
        if self.funded_at.get(&member.id).is_some_and(|at| at.elapsed() < FUNDING_WAIT) {
            return Some("net.wait_coins");
        }
        if !self.members.iter().any(|other| other.kind == Kind::Miner) {
            return Some("net.wait_miner");
        }
        let payer = self
            .members
            .iter()
            .filter(|other| other.kind == Kind::Miner && !other.busy)
            .filter_map(|other| Some((other.id, other.status.as_ref()?.spendable)))
            .filter(|(_, spendable)| *spendable > needs + COIN)
            .max_by_key(|(_, spendable)| *spendable);
        if let Some((payer, _)) = payer {
            self.send(payer, &format!("lab pay {address} {} 1", format_coins(needs)));
            self.asked.entry(payer).or_default().push_back(Asked { to: Some(member.id), amount: needs });
            self.funded_at.insert(member.id, Instant::now());
        }
        Some("net.wait_coins")
    }

    /// A sender pays the next receiver at its speed, payments a minute.
    fn send_payment(&mut self, member: &Member) -> Option<&'static str> {
        if member.paused {
            return None;
        }
        let receivers: Vec<(u32, String)> = self
            .members
            .iter()
            .filter(|other| other.kind == Kind::Receiver)
            .filter_map(|other| Some((other.id, other.address()?.to_string())))
            .collect();
        if receivers.is_empty() {
            return Some("net.wait_receiver");
        }
        let every = Duration::from_secs_f64(60.0 / member.speed.max(1) as f64);
        if self.paid_at.get(&member.id).is_some_and(|at| at.elapsed() < every) {
            return None;
        }
        let (to, address) = &receivers[(self.seq % receivers.len() as u64) as usize];
        self.send(member.id, &format!("lab pay {address} {} {SEND_RATE}", format_coins(SEND_AMOUNT)));
        self.asked.entry(member.id).or_default().push_back(Asked { to: Some(*to), amount: SEND_AMOUNT });
        self.paid_at.insert(member.id, Instant::now());
        None
    }

    fn spam(&mut self, member: &Member) -> Option<&'static str> {
        let rate = if member.paused { 0 } else { member.speed };
        self.tell(member.id, "spam", format!("lab spam {rate}"));
        None
    }

    /// A miner put in a pool works there once the pool listens; taken out, it mines alone again.
    fn join_pool(&mut self, member: &Member) {
        if member.busy {
            return;
        }
        let port = member.pool.and_then(|pool| self.member(pool)).and_then(|pool| pool.status.as_ref()?.lab.as_ref()?.pool_port);
        let line = match (member.pool, port, member.address()) {
            (Some(_), Some(port), Some(address)) => format!("lab pool join 127.0.0.1:{port} {}", worker_name(address, member.number)),
            (Some(_), _, _) => return,
            (None, ..) => "lab pool leave".to_string(),
        };
        if line == "lab pool leave" && !self.told.contains_key(&(member.id, "pool")) {
            return;
        }
        self.tell(member.id, "pool", line);
    }

    /// A node with no peer left, after one it dialed went, dials the first participant again.
    fn heal(&mut self) {
        if self.healed_at.is_some_and(|at| at.elapsed() < HEAL_EVERY) || self.members.len() < 2 {
            return;
        }
        self.healed_at = Some(Instant::now());
        let lonely: Vec<u32> = self
            .members
            .iter()
            .filter(|member| !member.busy && !member.paused)
            .filter(|member| member.status.as_ref().is_some_and(|status| status.peers == 0 && !status.offline))
            .map(|member| member.id)
            .collect();
        for id in lonely {
            let Some(hub) = self.members.iter().find(|other| other.id != id && other.port().is_some()).map(|other| other.id) else {
                continue;
            };
            if let Some(address) = self.address_of_member(hub) {
                self.send(id, &format!("lab connect {address}"));
                if let Some(member) = self.member_mut(id) {
                    if !member.links.contains(&hub) {
                        member.links.push(hub);
                    }
                }
            }
        }
    }

    /// The attacker attacks once a receiver and honest miners are there and it holds the coins it
    /// pays; afterwards it mines honestly until asked again.
    fn attack_when_ready(&mut self, member: &Member) -> Option<&'static str> {
        if member.busy || self.attacked.contains(&member.id) {
            return None;
        }
        if self.runs.running() {
            return Some("net.wait_run");
        }
        let Some(victim) = self.members.iter().find(|other| other.kind == Kind::Receiver && other.status.is_some()).map(|other| other.id) else {
            return Some("net.wait_receiver");
        };
        let public: Vec<u32> = self
            .members
            .iter()
            .filter(|other| other.kind == Kind::Miner && !other.paused && other.status.is_some())
            .map(|other| other.id)
            .collect();
        if public.is_empty() {
            return Some("net.wait_honest");
        }
        if member.status.as_ref().map_or(0, |status| status.spendable) < ATTACK_AMOUNT * COIN * 2 {
            return Some("net.wait_coins");
        }
        if member.paused {
            return None;
        }
        let helper =
            self.members.iter().find(|other| other.kind == Kind::Attacker && other.id != member.id && other.status.is_some()).map(|other| other.id);
        self.attacked.insert(member.id);
        if let Err(error) = self.start_attack(member.id, victim, helper, public) {
            self.error = Some(error);
        }
        None
    }

    /// The split of the one-click setup starts once its three miners agree on a chain.
    fn split_when_ready(&mut self) {
        if !self.split_asked || self.runs.running() {
            return;
        }
        let miners: Vec<&Member> = self.members.iter().filter(|member| member.kind == Kind::Miner).collect();
        let tips: Vec<Option<&str>> = miners.iter().map(|member| member.status.as_ref().map(|status| status.tip.as_str())).collect();
        let agreed = miners.len() >= 3 && tips.iter().all(|tip| tip.is_some() && *tip == tips[0]) && miners[0].height() >= 2;
        if !agreed {
            return;
        }
        let ids: Vec<u32> = miners.iter().map(|member| member.id).collect();
        self.split_asked = false;
        if let Err(error) = self.start_split(ids[..ids.len() - 1].to_vec(), ids[ids.len() - 1]) {
            self.error = Some(error);
        }
    }

    /// The bug exploiter tries each trick on every other node once, when it holds coins.
    fn tricks(&mut self, member: &Member) -> Option<&'static str> {
        if self.tried.contains(&member.id) || member.paused {
            return None;
        }
        // A light node keeps no blocks to build a trick on and checks no payment; a busy or
        // cut-off node is not there to judge.
        let targets: Vec<(u32, std::net::SocketAddr)> = self
            .members
            .iter()
            .filter(|other| other.id != member.id && other.port().is_some() && other.status.is_some())
            .filter(|other| other.kind != Kind::Light && !other.busy && !(other.paused && !other.kind.mines()))
            .filter_map(|other| Some((other.id, self.address_of_member(other.id)?.parse().ok()?)))
            .collect();
        if targets.is_empty() {
            return Some("net.wait_target");
        }
        self.tried.insert(member.id);
        let funds = self.coin.as_ref().and_then(|coin| coin.ancestors().nth(3)).map(|net| net.join("local").join(member.id.to_string()).join("wallet"));
        if let (Some(funds), Some(coin)) = (funds, self.config.clone()) {
            self.runs.start_tricks(member.id, coin, targets, funds);
        }
        None
    }
}
