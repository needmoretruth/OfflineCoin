//! Commands and answers for a running board. [`BoardLink`] is the socket: a cable or a packet
//! across the fleet, as long as it carries the same lines.
use crate::state::BoardState;
use olc_link_serial::SerialLink;
use std::collections::BTreeMap;
use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, Instant};

const ANSWER_WAIT: Duration = Duration::from_secs(3);
const LINE_WAIT: Duration = Duration::from_millis(200);
const SET_TRIES: u32 = 4;
const ASK_TRIES: u32 = 4;

pub trait BoardLink: Send {
    fn send(&mut self, line: &str) -> Result<(), String>;
    /// The next line the board printed, or `None` once `wait` has passed.
    fn receive(&mut self, wait: Duration) -> Option<String>;
}

pub struct UsbBoard {
    link: SerialLink,
    lines: Receiver<String>,
}

impl UsbBoard {
    pub fn open(path: &str) -> Result<Self, String> {
        let (sender, lines) = channel();
        let on_line = move |line: &str| {
            let _ = sender.send(line.to_string());
        };
        let link = SerialLink::open(path, on_line).map_err(|error| format!("{path}: {error}"))?;
        Ok(Self { link, lines })
    }
}

impl BoardLink for UsbBoard {
    fn send(&mut self, line: &str) -> Result<(), String> {
        self.link.send_line(line).map_err(|error| error.to_string())
    }

    fn receive(&mut self, wait: Duration) -> Option<String> {
        self.lines.recv_timeout(wait).ok()
    }
}

/// What the board's `status` command prints.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Status {
    pub height: u64,
    pub tip: String,
    pub work: u128,
    pub balance: String,
    pub pool: u64,
    pub hashrate: u64,
    pub limit: u64,
    /// The owner chose this cap, rather than the default standing in.
    pub limit_chosen: bool,
    pub mining: bool,
    pub offline: bool,
    pub view: Option<String>,
    /// The coin's frames a second on the panel.
    pub smooth: Option<u32>,
    /// Its own settings, as far as its firmware says them.
    pub state: BoardState,
    pub address: String,
    /// The last branch change as (height, blocks replaced).
    pub reorg: (u64, u64),
}

impl Status {
    /// The lines that bring a board back to this state.
    pub fn restore_lines(&self) -> Vec<String> {
        let limit = match self.limit_chosen {
            true => self.limit.to_string(),
            false => "default".to_string(),
        };
        let mut lines = vec![
            format!("offline {}", on_off(self.offline)),
            format!("limit {limit}"),
            format!("mine {}", on_off(self.mining)),
        ];
        lines.extend(self.view.iter().map(|view| format!("view {view}")));
        lines
    }

    fn parse(line: &str) -> Option<Self> {
        let fields: BTreeMap<&str, &str> = line
            .split_whitespace()
            .filter_map(|word| word.split_once('='))
            .collect();
        Some(Self {
            height: fields.get("height")?.parse().ok()?,
            tip: fields.get("tip")?.to_string(),
            work: fields.get("work")?.parse().ok()?,
            balance: fields.get("balance")?.to_string(),
            pool: fields.get("pool")?.parse().ok()?,
            hashrate: fields.get("hashrate")?.parse().ok()?,
            limit: fields.get("limit")?.parse().ok()?,
            limit_chosen: fields.get("chosen").is_some_and(|word| *word == "true"),
            mining: fields.get("mining")?.parse().ok()?,
            offline: fields.get("offline")?.parse().ok()?,
            view: fields.get("view").map(|view| view.to_string()),
            smooth: fields.get("smooth").and_then(|fps| fps.parse().ok()),
            state: BoardState::parse(&fields),
            address: fields.get("address")?.to_string(),
            reorg: fields
                .get("reorg")
                .and_then(|text| text.split_once('/'))
                .and_then(|(height, count)| Some((height.parse().ok()?, count.parse().ok()?)))
                .unwrap_or_default(),
        })
    }
}

/// Where a board sees a transaction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Place {
    Height(u64),
    Pool,
    /// Thrown out after a branch change, with the reason the board gave.
    Dropped(String),
    Nowhere,
}

pub struct Board {
    name: &'static str,
    link: Box<dyn BoardLink>,
}

impl Board {
    pub fn new(name: &'static str, link: Box<dyn BoardLink>) -> Self {
        Self { name, link }
    }

    pub fn send(&mut self, line: &str) -> Result<(), String> {
        self.link.send(line)
    }

    fn note(&mut self, line: &str) -> Result<(), String> {
        if line.starts_with("started ") {
            return Err(format!("The {} board restarted.", self.name));
        }
        Ok(())
    }

    /// Reads what the board printed earlier, so the next answer is not an old line.
    fn catch_up(&mut self) -> Result<(), String> {
        while let Some(line) = self.link.receive(Duration::ZERO) {
            self.note(&line)?;
        }
        Ok(())
    }

    /// Waits for a line that starts with one of `starts`. Reads everything else on the way.
    fn wait_for(&mut self, starts: &[&str]) -> Result<String, String> {
        let deadline = Instant::now() + ANSWER_WAIT;
        while Instant::now() < deadline {
            let Some(line) = self.link.receive(LINE_WAIT) else {
                continue;
            };
            self.note(&line)?;
            if starts.iter().any(|start| line.starts_with(start)) {
                return Ok(line);
            }
        }
        Err(format!("The {} board did not answer.", self.name))
    }

    /// Sends a line whose answer starts with `answer`, repeating it if it gets lost.
    pub fn command(&mut self, line: &str, answer: &str) -> Result<(), String> {
        self.ask(line, answer).map(|_| ())
    }

    /// Asks a question that is safe to repeat. A line on the cable can get lost either way.
    fn ask(&mut self, line: &str, answer: &str) -> Result<String, String> {
        let mut last = Err(format!("The {} board did not answer.", self.name));
        for _ in 0..ASK_TRIES {
            self.catch_up()?;
            self.send(line)?;
            last = self.wait_for(&[answer]);
            if last.is_ok() {
                break;
            }
        }
        last
    }

    pub fn status(&mut self) -> Result<Status, String> {
        // An empty line first: the first line after a restart can arrive garbled.
        let line = self.ask("\nstatus", "status ")?;
        Status::parse(&line).ok_or_else(|| format!("The {} board needs new firmware.", self.name))
    }

    /// For a moment the board is known to be busy: moving to another branch holds it for a while.
    pub fn status_within(&mut self, patience: Duration) -> Result<Status, String> {
        let deadline = Instant::now() + patience;
        loop {
            match self.status() {
                Err(_) if Instant::now() < deadline => {}
                answer => return answer,
            }
        }
    }

    /// Changes a setting and reads it back: a line sent to a busy board can get lost.
    pub fn set(&mut self, line: &str, holds: impl Fn(&Status) -> bool) -> Result<(), String> {
        for _ in 0..SET_TRIES {
            self.send(line)?;
            // Two reads: the first may have been printed before the line ran.
            self.status()?;
            if holds(&self.status()?) {
                return Ok(());
            }
        }
        Err(format!("The {} board did not take \"{line}\".", self.name))
    }

    pub fn set_mining(&mut self, on: bool) -> Result<(), String> {
        self.set(&format!("mine {}", on_off(on)), |now| now.mining == on)
    }

    pub fn set_offline(&mut self, on: bool) -> Result<(), String> {
        self.set(&format!("offline {}", on_off(on)), |now| now.offline == on)
    }

    pub fn set_limit(&mut self, limit: u64) -> Result<(), String> {
        self.set(&format!("limit {limit}"), |now| now.limit == limit)
    }

    /// Pays and returns the transaction's id.
    pub fn pay(&mut self, address: &str, amount: &str) -> Result<String, String> {
        self.catch_up()?;
        self.send(&format!("pay {address} {amount}"))?;
        let line = self.wait_for(&["paid ", "error "])?;
        match line.split_whitespace().collect::<Vec<_>>()[..] {
            ["paid", id, ..] => Ok(id.to_string()),
            _ => Err(format!("The {} board refused: {line}", self.name)),
        }
    }

    pub fn tx(&mut self, id: &str) -> Result<Place, String> {
        let line = self.ask(&format!("tx {id}"), &format!("tx {id} "))?;
        let place = line.rsplit(' ').next().unwrap_or_default();
        Ok(match place.strip_prefix("height=") {
            Some(height) => Place::Height(height.parse().map_err(|_| line.clone())?),
            None if place == "pool" => Place::Pool,
            None => match place.strip_prefix("dropped=") {
                Some(reason) => Place::Dropped(reason.to_string()),
                None => Place::Nowhere,
            },
        })
    }
}

pub fn on_off(on: bool) -> &'static str {
    if on {
        "on"
    } else {
        "off"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_reads_the_board_line() {
        let line = "status height=7 tip=ab work=99 difficulty=1 balance=1.50 peers=1 espnow=1 \
                    usb=0 wifi=0 pool=2 hashrate=300 limit=4 mining=true offline=false \
                    view=basic lang=en screen=true free=1 router=None address=olc00";
        let status = Status::parse(line).unwrap();
        assert_eq!(status.height, 7);
        assert_eq!(status.work, 99);
        assert_eq!(status.balance, "1.50");
        assert_eq!(status.limit, 4);
        assert!(status.mining && !status.offline);
    }
}
