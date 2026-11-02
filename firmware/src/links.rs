//! Commands about links, and the lines that say which link each neighbour is on.
use crate::node::{device_ms, Node};
use crate::radio;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicUsize, Ordering};
use olc_node::frame::MOST_ON_A_BOARD;
use olc_node::{Medium, Rank};

/// What a block's message carries beside the block.
const AROUND_A_BLOCK: u64 = 256;
static MOST_MESSAGE: AtomicUsize = AtomicUsize::new(MOST_ON_A_BOARD);

/// The longest message taken on a connection. Past this the connection ends.
pub fn most_message() -> usize {
    MOST_MESSAGE.load(Ordering::Relaxed)
}

/// A coin with blocks larger than the usual limit needs them to fit.
pub fn take_blocks_of(bytes: u64) {
    let bytes = usize::try_from(bytes.saturating_add(AROUND_A_BLOCK)).unwrap_or(usize::MAX);
    MOST_MESSAGE.store(bytes.max(MOST_ON_A_BOARD), Ordering::Relaxed);
}

impl Node {
    /// Turns a link on or off, or changes which is preferred.
    pub fn run_links_command(&mut self, line: &str) {
        let mut words = line.split_whitespace();
        match (words.next(), words.next()) {
            (Some("espnow"), Some(state)) => {
                radio::ENABLED.store(state == "on", Ordering::Relaxed);
                if state != "on" {
                    self.runner.link_off(Medium::EspNow);
                }
            }
            (Some("bluetooth"), Some(state @ ("on" | "off"))) => {
                self.restart_with(|settings| settings.bluetooth = state.into());
            }
            (Some("links"), None) => self.say(format!("links {}", self.runner.rank().text())),
            (Some("links"), Some(order)) => match Rank::parse(order) {
                Some(rank) => {
                    self.runner.set_rank(rank);
                    self.save_setting(|settings| settings.links = rank.text());
                    self.say(format!("links {}", rank.text()));
                }
                None => self.say("error links wired,espnow,bluetooth,wifi".into()),
            },
            (Some("wires"), Some("swap")) => crate::uart::swap(),
            (Some("wires"), Some(baud)) => match baud.parse().map(crate::uart::set_speed) {
                Ok(true) => self.say(format!("wires {baud}")),
                _ => self.say("error wires SPEED".into()),
            },
            (Some("wires"), None) => self.say(crate::uart::report()),
            (Some("near"), _) => self.print_neighbours(),
            (Some("length"), _) => self.say(format!("length {}", line.len())),
            _ => self.say("error links".into()),
        }
    }

    /// One line for each neighbour: who, which link carries its traffic, and what else reaches it.
    fn print_neighbours(&mut self) {
        let lines: Vec<String> = self
            .runner
            .neighbours()
            .iter()
            .map(|neighbour| {
                let medium = |link| self.runner.medium_of(link).map_or("?", Medium::name);
                let ways: Vec<&str> =
                    neighbour.paths.iter().filter(|path| path.live).map(|path| medium(path.link)).collect();
                // Its name still arrives, but nothing of the chain passes either way.
                let ignored = neighbour.node.is_some_and(radio::ignores_node);
                format!(
                    "node={:016x} on={} ways={} moves={}",
                    neighbour.node.unwrap_or(0),
                    if ignored { "ignored" } else { neighbour.active.map_or("none", medium) },
                    ways.join(","),
                    neighbour.moves,
                )
            })
            .collect();
        // One line, because a screen or a remote asker gets one answer to one question.
        self.say(format!("near {}", lines.join(" | ")));
    }

    /// Says when traffic left one link for another.
    pub fn report_moves(&mut self) {
        while let Some(moved) = self.runner.take_move() {
            let medium = |link| self.runner.medium_of(link).map_or("none", Medium::name);
            let line = format!(
                "link {:x} at={} from {} to {}",
                moved.peer,
                device_ms(),
                moved.from.map_or("none", medium),
                moved.to.map_or("none", medium),
            );
            self.say(line);
        }
    }
}
