//! A double spend on real boards. The attacker pays the victim, keeps its own chain out of sight
//! and spends the same coins on itself. Once its chain is heavier it comes back and the payment
//! is gone. Nothing stops this: the chain with the most work wins, which is the point of showing it.
use olc_board::{on_off, Args, Board, BoardLink, Place, Say, Status, UsbBoard};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

const POWER_SAMPLES: u32 = 5;
const PAY_TRIES: u32 = 3;
const RESTORE_TRIES: u32 = 4;

struct Plan {
    amount: String,
    confirmations: u64,
    victim_limit: u64,
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
            victim_limit: args.number("victim-limit", 5_000)?,
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
    let saved = (attacker.status()?, victim.status()?);
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
        say("Both boards are back to their cap and network state.".to_string());
    }
    result.and(restored)
}

fn restore(board: &mut Board, saved: &Status) -> Result<(), String> {
    let mut last = Err("The board did not answer.".to_string());
    for _ in 0..RESTORE_TRIES {
        let sent = board
            .send(&format!("offline {}", on_off(saved.offline)))
            .and_then(|()| board.send(&format!("limit {}", saved.limit)))
            .and_then(|()| board.send(&format!("mine {}", on_off(saved.mining))));
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
        self.victim.set_limit(self.plan.victim_limit)?;
        // The first second after a cap changes still counts the old rate.
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
            let (attacker, victim) = (self.attacker.status()?, self.victim.status()?);
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
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::Mutex;

    /// A board that answers `status` and remembers every line it was sent.
    #[derive(Clone, Default)]
    struct Fake {
        sent: Arc<Mutex<Vec<String>>>,
        replies: Arc<Mutex<VecDeque<String>>>,
        tip: &'static str,
        limit: Arc<Mutex<u64>>,
        idle: Arc<AtomicBool>,
        dark: Arc<AtomicBool>,
    }

    impl BoardLink for Fake {
        fn send(&mut self, line: &str) -> Result<(), String> {
            for line in line.lines().filter(|line| !line.is_empty()) {
                self.sent.lock().unwrap().push(line.to_string());
                if let Some(limit) = line.strip_prefix("limit ") {
                    *self.limit.lock().unwrap() = limit.parse().unwrap();
                }
                match line {
                    "mine on" => self.idle.store(false, Ordering::Relaxed),
                    "mine off" => self.idle.store(true, Ordering::Relaxed),
                    "offline on" => self.dark.store(true, Ordering::Relaxed),
                    "offline off" => self.dark.store(false, Ordering::Relaxed),
                    _ => {}
                }
                if line == "status" {
                    self.replies.lock().unwrap().push_back(format!(
                        "status height=9 tip={} work=5 difficulty=1 balance=500.00 peers=1 \
                         pool=0 hashrate=100 limit={} mining={} offline={} address=olc1",
                        self.tip,
                        self.limit.lock().unwrap(),
                        !self.idle.load(Ordering::Relaxed),
                        self.dark.load(Ordering::Relaxed)
                    ));
                }
            }
            Ok(())
        }

        fn receive(&mut self, _wait: Duration) -> Option<String> {
            self.replies.lock().unwrap().pop_front()
        }
    }

    fn quiet() -> Say {
        Arc::new(|_| {})
    }

    fn plan(timeout: Duration) -> Plan {
        Plan {
            amount: "10".into(),
            confirmations: 1,
            victim_limit: 777,
            timeout,
            poll: Duration::from_millis(1),
        }
    }

    #[test]
    fn a_failed_run_puts_both_boards_back() {
        // The tips never agree, so the run ends in `line_up`, after caps and `mine off` went out.
        let (attacker, victim) = (
            Fake {
                tip: "aa",
                ..Fake::default()
            },
            Fake {
                tip: "bb",
                ..Fake::default()
            },
        );
        *victim.limit.lock().unwrap() = 3_000;
        let (mut a, mut v) = (
            Board::new("attacker", Box::new(attacker.clone())),
            Board::new("victim", Box::new(victim.clone())),
        );
        let stop = AtomicBool::new(false);
        let result = attack(
            &mut a,
            &mut v,
            &plan(Duration::from_millis(40)),
            &quiet(),
            &stop,
        );
        assert!(result.is_err());
        let sent = victim.sent.lock().unwrap();
        assert!(sent.contains(&"limit 777".to_string()));
        let tail = |lines: &Vec<String>| {
            lines
                .iter()
                .rev()
                .filter(|l| *l != "status")
                .take(3)
                .cloned()
                .collect::<Vec<_>>()
        };
        assert_eq!(tail(&sent), ["mine on", "limit 3000", "offline off"]);
        assert_eq!(
            tail(&attacker.sent.lock().unwrap()),
            ["mine on", "limit 0", "offline off"]
        );
    }

    #[test]
    fn a_pressed_stop_ends_the_run_and_restores() {
        let (attacker, victim) = (
            Fake {
                tip: "aa",
                ..Fake::default()
            },
            Fake {
                tip: "aa",
                ..Fake::default()
            },
        );
        let (mut a, mut v) = (
            Board::new("attacker", Box::new(attacker.clone())),
            Board::new("victim", Box::new(victim.clone())),
        );
        let stop = AtomicBool::new(true);
        let result = attack(
            &mut a,
            &mut v,
            &plan(Duration::from_secs(60)),
            &quiet(),
            &stop,
        );
        assert_eq!(result, Err("stopped by hand.".to_string()));
        assert!(attacker
            .sent
            .lock()
            .unwrap()
            .contains(&"offline off".to_string()));
    }
}
