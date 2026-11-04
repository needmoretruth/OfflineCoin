//! A double spend on real boards. The attacker pays the victim, keeps its own chain out of sight
//! and spends the same coins on itself. Once its chain is heavier it comes back and the payment
//! is gone. Nothing stops this: the chain with the most work wins, which is the point of showing it.
//! A third board may help the attacker. Then every board stays at its own cap, and the victim is
//! the one cut off, because the two attacking boards must keep hearing each other.
//! The same steps run on miners on this computer, which answer a board's lines. Their caps are
//! set before the run, so the attacker holds the share a person chose, and an attacker under half
//! may give up and come back to a chain that kept the payment.
//! In a network of many, the victim may be a node that only receives, and the public chain is
//! mined by honest miners beside it: they stop and start where a mining victim would.
use crate::steps::{Outcome, Step, Watch};
use olc_board::journal::Journal;
use olc_board::{Args, Board, BoardLink, Place, Say, Status, UsbBoard};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

/// A board rebuilds its coins from the first block when it changes branch.
const BRANCH_MOVE_WAIT: Duration = Duration::from_secs(120);
const POWER_SAMPLES: u32 = 5;
const PAY_TRIES: u32 = 3;
const RESTORE_TRIES: u32 = 4;

struct Plan {
    amount: String,
    confirmations: u64,
    /// The victim's cap, or 0 for half of what the attacker's board really does.
    victim_limit: u64,
    journal: Journal,
    timeout: Duration,
    /// Time between looks at the boards.
    poll: Duration,
    /// Leaves every cap as it was set before the run.
    keep_caps: bool,
    /// The victim mines the public chain. Off, it only receives and watches.
    victim_mines: bool,
    /// The attacker mines itself. A pool does not: its crew mines for it.
    attacker_mines: bool,
    /// How many of the boards beside the victim are the attacker's crew, listed first.
    crew: usize,
    /// The attacker gives up this many blocks behind the public chain. 0 never gives up.
    give_up_behind: u64,
    /// The attacker gives up after racing this long.
    race_limit: Duration,
    /// How many blocks ahead the private chain must be before it comes back. With fast blocks a
    /// lead of one can be tied by a public block found while the hidden ones travel.
    reveal_lead: u64,
    watch: Watch,
}

/// A double spend on miners whose caps already hold the share a person chose.
pub struct AttackOptions {
    pub amount: String,
    pub confirmations: u64,
    pub journal: Journal,
    pub timeout: Duration,
    pub give_up_behind: u64,
    pub race_limit: Duration,
    /// Blocks the private chain must lead by before it comes back; 1 is as soon as it is heavier.
    pub reveal_lead: u64,
    /// Honest miners beside the victim, which mine the public chain.
    pub public: Vec<Box<dyn BoardLink>>,
    /// The victim mines too, as with two boards. Off, it only receives.
    pub victim_mines: bool,
    /// Miners whose work reaches the attacker, a pool's workers: their speed is the attacker's,
    /// and they mine and stop with it.
    pub crew: Vec<Box<dyn BoardLink>>,
    /// The attacker mines itself. A pool only builds blocks from its crew's work.
    pub attacker_mines: bool,
}

fn no_watch() -> Watch {
    Arc::new(|_| {})
}

impl Plan {
    fn from_args(args: &Args) -> Result<Self, String> {
        let amount = args.value("amount").unwrap_or("10").to_string();
        if !amount.parse::<f64>().is_ok_and(|coins| coins > 0.0) {
            return Err("--amount needs a number above 0".into());
        }
        Ok(Self {
            amount,
            confirmations: args.number("confirmations", 2)?,
            victim_limit: args.number("victim-limit", 0)?,
            journal: Journal::standard()?,
            timeout: Duration::from_secs(args.number("timeout", 1800)?),
            poll: Duration::from_secs(2),
            keep_caps: false,
            victim_mines: true,
            attacker_mines: true,
            crew: 0,
            give_up_behind: 0,
            race_limit: Duration::MAX,
            reveal_lead: 1,
            watch: no_watch(),
        })
    }
}

/// From the terminal, through two cables.
pub fn run(args: &Args) -> Result<(), String> {
    let attacker = args.value("attacker").ok_or("--attacker needs a port")?;
    let victim = args.value("victim").ok_or("--victim needs a port")?;
    let plan = Plan::from_args(args)?;
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    ctrlc::set_handler(move || flag.store(true, Ordering::Relaxed))
        .map_err(|error| error.to_string())?;
    let mut attacker = Board::new("attacker", Box::new(UsbBoard::open(attacker)?));
    let mut victim = Board::new("victim", Box::new(UsbBoard::open(victim)?));
    let say: Say = Arc::new(|line| println!("{line}"));
    attack(&mut attacker, &mut victim, None, &plan, &say, &stop).map(|_| ())
}

/// From the app, on boards reached any way at all.
pub fn run_on(
    boards: Vec<Box<dyn BoardLink>>,
    args: &Args,
    say: Say,
    stop: Arc<AtomicBool>,
) -> Result<(), String> {
    let plan = Plan::from_args(args)?;
    let mut boards = boards.into_iter();
    let (Some(attacker), Some(victim)) = (boards.next(), boards.next()) else {
        return Err("Two boards are needed.".into());
    };
    let mut attacker = Board::new("attacker", attacker);
    let mut victim = Board::new("victim", victim);
    let mut helper = boards.next().map(|link| Board::new("helper", link));
    attack(&mut attacker, &mut victim, helper.as_mut(), &plan, &say, &stop).map(|_| ())
}

/// On miners of this computer, or boards, whose caps are set already: attacker, victim, then
/// an optional helper. Each step goes to `watch` as it happens.
pub fn attack_with(
    links: Vec<Box<dyn BoardLink>>,
    options: AttackOptions,
    say: Say,
    watch: Watch,
    stop: Arc<AtomicBool>,
) -> Result<Outcome, String> {
    let plan = Plan {
        amount: options.amount,
        confirmations: options.confirmations,
        victim_limit: 0,
        journal: options.journal,
        timeout: options.timeout,
        poll: Duration::from_millis(500),
        keep_caps: true,
        victim_mines: options.victim_mines,
        attacker_mines: options.attacker_mines,
        crew: options.crew.len(),
        give_up_behind: options.give_up_behind,
        race_limit: options.race_limit,
        reveal_lead: options.reveal_lead.max(1),
        watch,
    };
    let mut links = links.into_iter();
    let (Some(attacker), Some(victim)) = (links.next(), links.next()) else {
        return Err("Two miners are needed.".into());
    };
    let mut attacker = Board::new("attacker", attacker);
    let mut victim = Board::new("victim", victim);
    let mut helper = links.next().map(|link| Board::new("helper", link));
    let crew = options.crew.into_iter().map(|link| Board::new("crew", link));
    let mut public: Vec<Board> = crew.chain(options.public.into_iter().map(|link| Board::new("honest", link))).collect();
    attack_among(&mut attacker, &mut victim, helper.as_mut(), &mut public, &plan, &say, &stop)
}

/// Runs the steps, then puts both boards back as they were, whatever happened.
fn attack(
    attacker: &mut Board,
    victim: &mut Board,
    helper: Option<&mut Board>,
    plan: &Plan,
    say: &Say,
    stop: &AtomicBool,
) -> Result<Outcome, String> {
    attack_among(attacker, victim, helper, &mut [], plan, say, stop)
}

/// The same with other miners, put back as they were too: the attacker's crew first, then the
/// honest miners beside the victim.
fn attack_among(
    attacker: &mut Board,
    victim: &mut Board,
    mut helper: Option<&mut Board>,
    public: &mut [Board],
    plan: &Plan,
    say: &Say,
    stop: &AtomicBool,
) -> Result<Outcome, String> {
    put_back_leftovers(attacker, plan, say)?;
    put_back_leftovers(victim, plan, say)?;
    let saved = (attacker.status()?, victim.status()?);
    let helper_saved = match helper.as_deref_mut() {
        Some(board) => {
            put_back_leftovers(board, plan, say)?;
            Some(board.status()?)
        }
        None => None,
    };
    let mut public_saved = Vec::new();
    for board in public.iter_mut() {
        put_back_leftovers(board, plan, say)?;
        public_saved.push(board.status()?);
    }
    for board in [&saved.0, &saved.1].into_iter().chain(&helper_saved).chain(&public_saved) {
        plan.journal
            .remember(&board.address, board.restore_lines())?;
    }
    let (crew, honest) = public.split_at_mut(plan.crew.min(public.len()));
    let mut race = Race {
        attacker,
        victim,
        helper: helper.as_deref_mut(),
        crew,
        public: honest,
        plan,
        say,
        stop,
        deadline: Instant::now() + plan.timeout,
    };
    let result = race.steps(&saved.0, &saved.1);
    if let Err(error) = &result {
        say(format!("Stopped: {error}"));
    }
    let helper_back = match (helper, &helper_saved) {
        (Some(board), Some(saved)) => restore(board, saved),
        _ => Ok(()),
    };
    let public_back = public.iter_mut().zip(&public_saved).try_for_each(|(board, saved)| restore(board, saved));
    let restored = restore(attacker, &saved.0)
        .and(restore(victim, &saved.1))
        .and(helper_back)
        .and(public_back);
    if restored.is_ok() {
        for board in [&saved.0, &saved.1].into_iter().chain(&helper_saved).chain(&public_saved) {
            plan.journal.forget(&board.address);
        }
        say("All boards are back to their cap and network state.".to_string());
    }
    result.and_then(|outcome| restored.map(|()| outcome))
}

fn short(id: &str) -> &str {
    id.get(..12).unwrap_or(id)
}

macro_rules! say {
    ($race:expr, $($text:tt)*) => {
        ($race.say)(format!($($text)*))
    };
}

struct Race<'a> {
    attacker: &'a mut Board,
    victim: &'a mut Board,
    helper: Option<&'a mut Board>,
    /// Miners whose work goes to the attacker.
    crew: &'a mut [Board],
    /// Honest miners beside the victim.
    public: &'a mut [Board],
    plan: &'a Plan,
    say: &'a Say,
    stop: &'a AtomicBool,
    deadline: Instant,
}

impl Race<'_> {
    fn pause(&self) -> Result<(), String> {
        if self.stop.load(Ordering::Relaxed) {
            return Err("stopped by hand.".into());
        }
        if Instant::now() > self.deadline {
            return Err("out of time.".into());
        }
        thread::sleep(self.plan.poll);
        Ok(())
    }

    fn steps(&mut self, attacker: &Status, victim: &Status) -> Result<Outcome, String> {
        say!(
            self,
            "Attacker {} at height {}",
            attacker.address,
            attacker.height
        );
        say!(
            self,
            "Victim   {} at height {}",
            victim.address,
            victim.height
        );
        let spendable = attacker.balance.parse::<f64>().unwrap_or(0.0);
        if spendable < self.plan.amount.parse().unwrap_or(f64::MAX) {
            return Err(format!("the attacker holds only {} OLC.", attacker.balance));
        }
        self.measure_power()?;
        self.line_up()?;
        let before = self.victim.status()?.balance;
        let payment = self.pay_victim(&victim.address, &before)?;
        self.go_dark()?;
        // The hidden chain grows from the moment the attacker is out of sight, while the victim
        // waits for its confirmations, as a real double spend does.
        let spend = self.spend_again(&attacker.address)?;
        let confirmed = self.confirm(&payment)?;
        if !self.outrun(&spend)? {
            return self.come_back_behind(&payment);
        }
        self.come_back()?;
        self.refused(&payment)?;
        self.judge(&payment, &spend, [&before, &confirmed])?;
        (self.plan.watch)(Step::Gone);
        Ok(Outcome::Gone)
    }

    fn measure_power(&mut self) -> Result<(), String> {
        say!(self, "\n1. Hash power");
        let together = self.helper.is_some() || self.plan.keep_caps;
        for (board, mines) in [(&mut *self.attacker, self.plan.attacker_mines), (&mut *self.victim, self.plan.victim_mines)] {
            if !together {
                board.set_limit(0)?;
            }
            if mines {
                board.set_mining(true)?;
            }
            board.set_offline(false)?;
        }
        for board in self.public.iter_mut().chain(self.crew.iter_mut()) {
            board.set_mining(true)?;
            board.set_offline(false)?;
        }
        if let Some(helper) = &mut self.helper {
            helper.set_mining(true)?;
            helper.set_offline(false)?;
        }
        // The first second after a cap changes still counts the old rate.
        self.pause()?;
        self.pause()?;
        if !together {
            let cap = match self.plan.victim_limit {
                0 => (self.attacker.status()?.hashrate / 2).max(1),
                chosen => chosen,
            };
            self.victim.set_limit(cap)?;
            self.pause()?;
            self.pause()?;
        }
        let (mut attacker, mut victim) = (0, 0);
        for _ in 0..POWER_SAMPLES {
            self.pause()?;
            attacker += self.attacker.status()?.hashrate;
            for board in self.crew.iter_mut() {
                attacker += board.status()?.hashrate;
            }
            if let Some(helper) = &mut self.helper {
                attacker += helper.status()?.hashrate;
            }
            victim += self.victim.status()?.hashrate;
            for board in self.public.iter_mut() {
                victim += board.status()?.hashrate;
            }
        }
        let total = (attacker + victim).max(1) as f64;
        let share = |rate: u64| rate as f64 * 100.0 / total;
        let samples = u64::from(POWER_SAMPLES);
        say!(
            self,
            "   attacker {} H/s, {:.1}% of all hashing",
            attacker / samples,
            share(attacker)
        );
        say!(
            self,
            "   victim   {} H/s, {:.1}% of all hashing",
            victim / samples,
            share(victim)
        );
        (self.plan.watch)(Step::Power {
            attacker: attacker / samples,
            victim: victim / samples,
        });
        Ok(())
    }

    /// Stops both miners on the same tip, so no block can slip in around the payment. Two tips
    /// of equal work never settle by themselves, so the attacker mines until one is heavier.
    fn line_up(&mut self) -> Result<(), String> {
        if self.plan.victim_mines {
            self.victim.set_mining(false)?;
        }
        for board in self.public.iter_mut() {
            board.set_mining(false)?;
        }
        if let Some(helper) = &mut self.helper {
            helper.set_mining(false)?;
        }
        loop {
            if self.same_tip()? {
                self.side_mining(false)?;
                if self.same_tip()? {
                    let height = self.attacker.status()?.height;
                    say!(self, "\n   Both boards agree at height {height}");
                    (self.plan.watch)(Step::LinedUp { height });
                    return Ok(());
                }
                self.side_mining(true)?;
            }
            self.pause()?;
        }
    }

    /// The attacker's side mines or stops: the attacker when it mines itself, and its crew.
    fn side_mining(&mut self, on: bool) -> Result<(), String> {
        if self.plan.attacker_mines {
            self.attacker.set_mining(on)?;
        }
        self.crew.iter_mut().try_for_each(|board| board.set_mining(on))
    }

    fn same_tip(&mut self) -> Result<bool, String> {
        Ok(self.attacker.status()?.tip == self.victim.status()?.tip)
    }

    fn pay_victim(&mut self, address: &str, balance: &str) -> Result<String, String> {
        say!(
            self,
            "\n2. The attacker pays the victim {} OLC",
            self.plan.amount
        );
        say!(self, "   victim balance {balance} OLC");
        for _ in 0..PAY_TRIES {
            let id = self.attacker_pays(address)?;
            for _ in 0..POWER_SAMPLES {
                if self.victim.tx(&id)? == Place::Pool {
                    say!(self, "   payment {} reached the victim", short(&id));
                    (self.plan.watch)(Step::Paid { payment: id.clone() });
                    return Ok(id);
                }
                self.pause()?;
            }
        }
        Err("the payment did not reach the victim.".into())
    }

    /// A radio broadcast or a line on the cable can be lost. The same coins give the same payment,
    /// so paying again after clearing the pool is safe.
    fn attacker_pays(&mut self, address: &str) -> Result<String, String> {
        let mut last = Err("the attacker did not pay.".to_string());
        for _ in 0..PAY_TRIES {
            self.attacker.send("pool clear")?;
            last = self.attacker.pay(address, &self.plan.amount);
            if last.is_ok() {
                break;
            }
        }
        last
    }

    /// Dark comes first: a board that saw the confirming block could not mine without it.
    fn go_dark(&mut self) -> Result<(), String> {
        say!(self, "\n3. The attacker goes dark");
        match &mut self.helper {
            Some(helper) => {
                self.victim.set_offline(true)?;
                helper.set("pool clear", |now| now.pool == 0)?;
            }
            None => self.attacker.set_offline(true)?,
        }
        self.attacker.set("pool clear", |now| now.pool == 0)?;
        if self.plan.victim_mines {
            self.victim.set_mining(true)?;
        }
        for board in self.public.iter_mut() {
            board.set_mining(true)?;
        }
        (self.plan.watch)(Step::Dark);
        Ok(())
    }

    /// Returns the victim's balance once the payment is deep enough.
    fn confirm(&mut self, payment: &str) -> Result<String, String> {
        say!(self, "\n5. The victim mines the payment");
        let mut seen = 0;
        loop {
            let status = self.victim.status()?;
            if status.height != seen {
                seen = status.height;
                if let Place::Height(height) = self.victim.tx(payment)? {
                    let depth = status.height + 1 - height;
                    say!(
                        self,
                        "   payment at height {height}, {depth} of {} confirmations",
                        self.plan.confirmations
                    );
                    (self.plan.watch)(Step::Confirmed {
                        depth: depth.min(self.plan.confirmations),
                        of: self.plan.confirmations,
                    });
                    if depth >= self.plan.confirmations {
                        let balance = self.victim.status()?.balance;
                        say!(self, "   payment confirmed, victim balance {balance} OLC");
                        return Ok(balance);
                    }
                }
            }
            self.pause()?;
        }
    }

    fn spend_again(&mut self, own_address: &str) -> Result<String, String> {
        say!(
            self,
            "\n4. The attacker spends the same coins on itself, out of sight"
        );
        let id = self.attacker_pays(own_address)?;
        say!(self, "   private spend {}", short(&id));
        (self.plan.watch)(Step::SpentAgain { spend: id.clone() });
        self.side_mining(true)?;
        if let Some(helper) = &mut self.helper {
            helper.set_mining(true)?;
        }
        Ok(id)
    }

    /// True once the private chain is heavier and holds the spend. False when the attacker
    /// gives up: too far behind, or racing too long.
    fn outrun(&mut self, spend: &str) -> Result<bool, String> {
        say!(self, "\n6. The private chain against the public chain");
        let mut shown = (0, 0);
        let started = Instant::now();
        loop {
            let (attacker, victim) = (self.attacker.status()?, self.victim.status()?);
            if (attacker.height, victim.height) != shown {
                shown = (attacker.height, victim.height);
                say!(
                    self,
                    "   attacker height {} work {} at {} H/s | victim height {} work {} at {} H/s",
                    attacker.height,
                    attacker.work,
                    attacker.hashrate,
                    victim.height,
                    victim.work,
                    victim.hashrate
                );
            }
            let ahead = attacker.height >= victim.height + self.plan.reveal_lead;
            let heavier = attacker.work > victim.work && ahead;
            if heavier && matches!(self.attacker.tx(spend)?, Place::Height(_)) {
                say!(self, "   the private chain is heavier");
                (self.plan.watch)(Step::Heavier);
                return Ok(true);
            }
            let behind = victim.height.saturating_sub(attacker.height);
            let far = self.plan.give_up_behind > 0 && behind >= self.plan.give_up_behind;
            if far || started.elapsed() > self.plan.race_limit {
                say!(self, "   the attacker gives up, {behind} blocks behind");
                (self.plan.watch)(Step::GaveUp { behind });
                return Ok(false);
            }
            self.pause()?;
        }
    }
}

mod back;
mod keep;
use keep::{put_back_leftovers, restore};
#[cfg(test)]
mod tests;
