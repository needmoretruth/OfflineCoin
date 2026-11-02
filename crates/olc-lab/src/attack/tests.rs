use super::*;
use std::collections::VecDeque;
use std::sync::atomic::AtomicUsize;
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
                     pool=0 hashrate=100 limit={} chosen=true mining={} offline={} address=olc1",
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

static NEXT_JOURNAL: AtomicUsize = AtomicUsize::new(0);

fn quiet() -> Say {
    Arc::new(|_| {})
}

fn plan(timeout: Duration) -> Plan {
    Plan {
        amount: "10".into(),
        confirmations: 1,
        victim_limit: 777,
        journal: Journal::at(std::env::temp_dir().join(format!(
            "olc-lab-journal-{}-{}",
            std::process::id(),
            NEXT_JOURNAL.fetch_add(1, Ordering::Relaxed),
        ))),
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
        None,
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
fn a_helper_leaves_every_cap_alone_and_is_put_back() {
    let (attacker, victim, helper) = (
        Fake {
            tip: "aa",
            ..Fake::default()
        },
        Fake {
            tip: "bb",
            ..Fake::default()
        },
        Fake {
            tip: "aa",
            ..Fake::default()
        },
    );
    for board in [&attacker, &victim, &helper] {
        *board.limit.lock().unwrap() = 1_000;
    }
    let (mut a, mut v, mut h) = (
        Board::new("attacker", Box::new(attacker.clone())),
        Board::new("victim", Box::new(victim.clone())),
        Board::new("helper", Box::new(helper.clone())),
    );
    let stop = AtomicBool::new(false);
    let result = attack(
        &mut a,
        &mut v,
        Some(&mut h),
        &plan(Duration::from_millis(40)),
        &quiet(),
        &stop,
    );
    assert!(result.is_err());
    for board in [&attacker, &victim, &helper] {
        let sent = board.sent.lock().unwrap();
        assert!(!sent.contains(&"limit 0".to_string()));
    }
    let sent = helper.sent.lock().unwrap();
    assert!(sent.contains(&"mine off".to_string()));
    assert!(sent.contains(&"mine on".to_string()));
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
    let result = attack(&mut a, &mut v, None, &plan(Duration::from_secs(60)),
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
