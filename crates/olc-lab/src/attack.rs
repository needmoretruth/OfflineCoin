//! A double spend on real boards. The attacker pays the victim, keeps its own chain out of sight
//! and spends the same coins on itself. Once its chain is heavier it comes back and the payment
//! is gone. Nothing stops this: the chain with the most work wins, which is the point of showing it.
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
    attack(&mut attacker, &mut victim, &plan, &say, &stop)
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
    attack(&mut attacker, &mut victim, &plan, &say, &stop)
}

/// Runs the steps, then puts both boards back as they were, whatever happened.
fn attack(
    attacker: &mut Board,
    victim: &mut Board,
    plan: &Plan,
    say: &Say,
    stop: &AtomicBool,
) -> Result<(), String> {
    put_back_leftovers(attacker, plan, say)?;
    put_back_leftovers(victim, plan, say)?;
    let saved = (attacker.status()?, victim.status()?);
    for board in [&saved.0, &saved.1] {
        plan.journal
            .remember(&board.address, board.restore_lines())?;
    }
    let mut race = Race {
        attacker,
        victim,
        plan,
        say,
        stop,
        deadline: Instant::now() + plan.timeout,
    };
    let result = race.steps(&saved.0, &saved.1);
    if let Err(error) = &result {
        say(format!("Stopped: {error}"));
    }
    let restored = restore(attacker, &saved.0).and(restore(victim, &saved.1));
    if restored.is_ok() {
        plan.journal.forget(&saved.0.address);
        plan.journal.forget(&saved.1.address);
        say("Both boards are back to their cap and network state.".to_string());
    }
    result.and(restored)
}

/// A run that died left this board changed, so it starts from its true state.
fn put_back_leftovers(board: &mut Board, plan: &Plan, say: &Say) -> Result<(), String> {
    let address = board.status()?.address;
    let left = plan.journal.pending();
    let Some(entry) = left.iter().find(|entry| entry.address == address) else {
        return Ok(());
    };
    entry.lines.iter().try_for_each(|line| board.send(line))?;
    plan.journal.forget(&address);
    say("A run that died left this board changed. It is back as it was.".to_string());
    Ok(())
}

fn restore(board: &mut Board, saved: &Status) -> Result<(), String> {
    let mut last = Err("The board did not answer.".to_string());
    for _ in 0..RESTORE_TRIES {
        let sent = saved
            .restore_lines()
            .iter()
            .try_for_each(|line| board.send(line));
        last = sent.and_then(|()| {
            // Two reads: the first may have been printed before the commands ran.
            board.status()?;
            board.status()
        });
        if let Ok(now) = &last {
            if (now.offline, now.limit, now.mining) == (saved.offline, saved.limit, saved.mining) {
                return Ok(());
            }
        }
    }
    last.and(Err(
        "A board did not go back to its saved state.".to_string()
    ))
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

    fn steps(&mut self, attacker: &Status, victim: &Status) -> Result<(), String> {
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
        let confirmed = self.confirm(&payment)?;
        let spend = self.spend_again(&attacker.address)?;
        self.outrun(&spend)?;
        self.come_back()?;
        self.refused(&payment)?;
        self.judge(&payment, &spend, [&before, &confirmed])
    }

    fn measure_power(&mut self) -> Result<(), String> {
        say!(self, "\n1. Hash power");
        for board in [&mut *self.attacker, &mut *self.victim] {
            board.set_limit(0)?;
            board.set_mining(true)?;
            board.set_offline(false)?;
        }
        // The first second after a cap changes still counts the old rate.
        self.pause()?;
        self.pause()?;
        let cap = match self.plan.victim_limit {
            0 => (self.attacker.status()?.hashrate / 2).max(1),
            chosen => chosen,
        };
        self.victim.set_limit(cap)?;
        self.pause()?;
        self.pause()?;
        let (mut attacker, mut victim) = (0, 0);
        for _ in 0..POWER_SAMPLES {
            self.pause()?;
            attacker += self.attacker.status()?.hashrate;
            victim += self.victim.status()?.hashrate;
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
        Ok(())
    }

    /// Stops both miners on the same tip, so no block can slip in around the payment. Two tips
    /// of equal work never settle by themselves, so the attacker mines until one is heavier.
    fn line_up(&mut self) -> Result<(), String> {
        self.victim.set_mining(false)?;
        loop {
            if self.same_tip()? {
                self.attacker.set_mining(false)?;
                if self.same_tip()? {
                    let height = self.attacker.status()?.height;
                    say!(self, "\n   Both boards agree at height {height}");
                    return Ok(());
                }
                self.attacker.set_mining(true)?;
            }
            self.pause()?;
        }
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
        self.attacker.set_offline(true)?;
        self.attacker.set("pool clear", |now| now.pool == 0)?;
        self.victim.set_mining(true)?;
        Ok(())
    }

    /// Returns the victim's balance once the payment is deep enough.
    fn confirm(&mut self, payment: &str) -> Result<String, String> {
        say!(self, "\n4. The victim mines the payment");
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
            "\n5. The attacker spends the same coins on itself, out of sight"
        );
        let id = self.attacker_pays(own_address)?;
        say!(self, "   private spend {}", short(&id));
        self.attacker.set_mining(true)?;
        Ok(id)
    }

    fn outrun(&mut self, spend: &str) -> Result<(), String> {
        say!(self, "\n6. The private chain against the public chain");
        let mut shown = (0, 0);
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
            let heavier = attacker.work > victim.work;
            if heavier && matches!(self.attacker.tx(spend)?, Place::Height(_)) {
                say!(self, "   the private chain is heavier");
                return Ok(());
            }
            self.pause()?;
        }
    }

    fn come_back(&mut self) -> Result<(), String> {
        say!(self, "\n7. The attacker comes back");
        let earlier = self.victim.status()?.reorg;
        self.attacker.set_offline(false)?;
        loop {
            let attacker = self.attacker.status()?;
            let victim = self.victim.status_within(BRANCH_MOVE_WAIT)?;
            if attacker.tip == victim.tip {
                if victim.reorg == earlier {
                    return Err("the victim's own chain won the race.".into());
                }
                let (height, replaced) = victim.reorg;
                say!(
                    self,
                    "   the victim dropped {replaced} blocks at height {height}"
                );
                return Ok(());
            }
            self.pause()?;
        }
    }

    /// Bitcoin's rule: a dropped block's payments go back through the pool's check. This one fails
    /// only because the coins are gone, which is what makes the attack a double spend.
    fn refused(&mut self, payment: &str) -> Result<(), String> {
        say!(
            self,
            "\n8. The payment goes back to the pool and is refused"
        );
        match self.victim.tx(payment)? {
            Place::Dropped(reason) if reason == "Some(MissingInput)" => {
                say!(
                    self,
                    "   the dropped blocks gave payment {} back",
                    short(payment)
                );
                say!(
                    self,
                    "   refused: its coins are already spent on the winning chain"
                );
                Ok(())
            }
            Place::Dropped(other) => Err(format!("the payment was refused for {other}.")),
            _ => Err("the victim did not refuse the payment.".into()),
        }
    }

    fn judge(
        &mut self,
        payment: &str,
        spend: &str,
        [before, confirmed]: [&str; 2],
    ) -> Result<(), String> {
        say!(self, "\n9. Result");
        let after = self.victim.status()?;
        let spent = match self.victim.tx(spend)? {
            Place::Height(height) => height,
            _ => return Err("the private spend is not on the winning chain.".into()),
        };
        let gone = |place| matches!(place, Place::Nowhere | Place::Dropped(_));
        if !gone(self.victim.tx(payment)?) || !gone(self.attacker.tx(payment)?) {
            return Err("the payment is still on the winning chain.".into());
        }
        say!(self, "   winning chain height {}", after.height);
        say!(self, "   payment: gone");
        say!(self, "   private spend: at height {spent}");
        say!(
            self,
            "   victim balance: {before} before, {confirmed} confirmed, {} now",
            after.balance
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests;
