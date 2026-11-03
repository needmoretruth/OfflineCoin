//! Lines typed into the app's console. The words and what they reach are the terminal's own: a
//! typed line ends in the same [`Command`] as the button beside it.
use super::commands::{Command, Personal};
use super::service::Core;
use super::host_locks::plain_detail;
use super::snapshot::{BoardView, Notice, Snapshot};
use super::words::{run_line, usage, Desk};
use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

/// A board answers within this, or never.
const ANSWER_WAIT: Duration = Duration::from_secs(12);
/// Long enough for the slowest own order, a password's hash on a busy computer.
const ORDER_WAIT: Duration = Duration::from_secs(20);
const LOOK_EVERY: Duration = Duration::from_millis(100);
/// Time for a lookup to reach the snapshot. It gives no notice to wait for.
const LOOKUP_SHOWS: Duration = Duration::from_millis(900);

const NO_ANSWER: &str = "Sent. No answer came back yet.";

/// Looks `text` up and gives what came back.
pub(super) fn looked_up(core: &Core, text: String) -> String {
    core.send(Command::Own(Personal::Explore(text.clone())));
    thread::sleep(LOOKUP_SHOWS);
    core.snapshot().node.explored.text(&text)
}

impl Command {
    /// What to say for an order that gives no notice, said as soon as it is sent.
    pub fn said_at_once(&self) -> Option<String> {
        quiet(self)
    }
}

fn quiet(command: &Command) -> Option<String> {
    Some(match command {
        Command::SetMining(true) => "Mining is on.".into(),
        Command::SetMining(false) => "Mining is off.".into(),
        Command::SetCap(0) => "No cap on hashes.".into(),
        Command::SetCap(cap) => format!("Up to {cap} hashes a second."),
        Command::AddAddress(address) => format!("This node will look for {}.", address.trim()),
        Command::Rescan => "Looking for nodes now.".into(),
        Command::StopNode(id) => format!("Node {id} stops."),
        Command::RemoveNode(id) => format!("Node {id} is removed."),
        Command::NodeCap { .. } | Command::NodeMining { .. } => "Saved.".into(),
        Command::NodeMiner { id, .. } => format!("Told node {id}."),
        Command::StopPool => "The pool stops.".into(),
        Command::StopExperiment => "The experiment stops.".into(),
        _ => return None,
    })
}

/// Sends `command` and waits for the notice it causes.
pub(super) fn answer_to(
    snapshot: &dyn Fn() -> Arc<Snapshot>,
    send: &dyn Fn(Command),
    command: Command,
) -> Result<String, String> {
    let board = match command {
        Command::Board { id, .. } => Some(id),
        _ => None,
    };
    let number = snapshot().notice.as_ref().map_or(0, |notice| notice.number);
    if let Some(said) = quiet(&command) {
        send(command);
        return Ok(said);
    }
    send(command);
    let started = Instant::now();
    let wait = if board.is_some() { ANSWER_WAIT } else { ORDER_WAIT };
    while started.elapsed() < wait {
        let seen = snapshot();
        // Records still opening is no answer: the order runs once they are in.
        let fresh = |notice: &&Notice| notice.number > number && notice.key != "notice.held";
        if let Some(notice) = seen.notice.as_ref().filter(fresh) {
            let said = notice.sentence();
            if !notice.good {
                return Err(said);
            }
            let asked = seen.boards.iter().find(|view| Some(view.seen.id) == board);
            let answer = asked.and_then(|view| view.answer.clone());
            return Ok(answer.unwrap_or(said));
        }
        thread::sleep(LOOK_EVERY);
    }
    Ok(NO_ANSWER.to_string())
}

/// What the command line says for each notice. `{}` takes the notice's detail.
const SENTENCES: &[(&str, &str)] = &[
    ("notice.backed_up", "Backed up to {}."),
    ("notice.backup_failed", "The backup was not saved."),
    ("notice.bad_address", "That address does not read right."),
    ("notice.bad_key", "That key does not read right."),
    ("notice.bad_amount", "That amount does not read right. Write a number above zero."),
    ("notice.board_done", "The board did that."),
    ("notice.board_failed", "The board did not finish. Try again."),
    ("notice.board_needs_cable", "Plug the board into this computer with its cable, then try again."),
    ("notice.board_refused", "The board said no."),
    ("notice.board_silent", "The board did not answer. Check that it is on."),
    ("notice.broadcast", "{}"),
    ("notice.coin_file", "That coin file did not work."),
    ("notice.coin_saved", "Coin file saved: {}"),
    ("notice.experiment_busy", "An experiment is already running."),
    ("notice.export_failed", "The key was not saved."),
    ("notice.fork_refused", "The rules did not change: {}"),
    ("notice.forked", "New rules from block {}."),
    ("notice.forked_back", "Back on the earlier rules of {}."),
    ("notice.held", "Your records are still opening."),
    ("notice.import_failed", "That secret key does not read right."),
    ("notice.install_refused", "That board is busy. Wait for it to finish."),
    ("notice.joined", "Joined {}."),
    ("notice.key_added", "{}"),
    ("notice.key_chosen", "Receiving address changed."),
    ("notice.key_failed", "The address did not change."),
    ("notice.key_saved", "Saved to {}."),
    ("notice.keys", "{}"),
    ("notice.known_key", "This wallet already holds that key."),
    ("notice.listen_failed", "Who may connect did not change."),
    ("notice.miner_set", "Mined coins go to {}."),
    ("notice.name_bad", "Type a name for the board."),
    ("notice.name_too_long", "That name is too long. A board keeps 32 bytes, about 10 Korean letters."),
    ("notice.needs_password", "This backup has a password. Add --password."),
    ("notice.no_bluetooth", "This computer cannot use Bluetooth."),
    ("notice.no_such_key", "This wallet has no such key."),
    ("notice.no_watched", "Watch an address first. The payment is made from its coins."),
    ("notice.node_failed", "The node did not start."),
    ("notice.node_paid", "Node {} sent the payment."),
    ("notice.not_a_backup", "That file is not a backup."),
    ("notice.not_changed", "That did not change."),
    ("notice.not_enough", "There are not enough coins to send that."),
    ("notice.note_failed", "The note was not sent."),
    ("notice.noted", "Note sent: {}"),
    ("notice.offline_failed", "That payment did not work."),
    ("notice.pay_failed", "The payment did not go out. Try again in a moment."),
    ("notice.pay_refused", "The payment could not join the queue. Send less, or try after the next block."),
    ("notice.prepared", "{}"),
    ("notice.radio_failed", "A radio did not start."),
    ("notice.restore_failed", "The backup was not read."),
    ("notice.restored", "{} addresses restored."),
    ("notice.save_failed", "Settings were not saved. Check that the disk has room."),
    ("notice.saved", "Saved."),
    ("notice.sent", "Sent {}."),
    ("notice.signed", "{}"),
    ("notice.snapshot_failed", "The list of coins was not taken."),
    ("notice.snapshot_taking", "Receiving the list of coins."),
    ("notice.wifi_name_bad", "Type the Wi-Fi name."),
    ("notice.wifi_name_too_long", "That Wi-Fi name is too long. Wi-Fi allows 32 bytes."),
    ("notice.wifi_password_bad", "A Wi-Fi password has 8 to 63 bytes."),
    ("notice.wrong_password", "That password is wrong."),
    ("notice.zero_amount", "The amount has to be more than zero."),
    ("lock.bad_text", "That text is not a spend."),
    ("lock.immature", "Mined coins can move only after more blocks."),
    ("lock.needs_blocks", "A node with headers only cannot read locks. Run olc light off."),
    ("lock.no_coin", "No coin under a lock there."),
    ("lock.no_key", "This wallet has no key this spend needs."),
    ("lock.no_words", "Write the lock first."),
    ("lock.not_open", "These unlock words do not open the lock. A time lock opens only from its height."),
    ("lock.not_taken", "Nodes would not take this spend."),
    ("lock.not_yet", "This lock opens only after more blocks. Try again then."),
    ("lock.open_branch", "IF, ELSE and ENDIF do not match."),
    ("lock.open_quote", "A quote is not closed."),
    ("lock.paid", "Coins are under the lock: {}"),
    ("lock.sent", "Spend sent: {}"),
    ("lock.spent", "This coin is already spent."),
    ("lock.too_long_word", "{} is too long for one word."),
    ("lock.too_many_bytes", "{} bytes. The most a lock takes is 1,000."),
    ("lock.too_many_checks", "{} signature checks. Nodes pass on 32 at most, so these coins could never be spent."),
    ("lock.too_many_ops", "{} words. The most a lock takes is 200."),
    ("lock.too_small", "The amount has to be more than the fee."),
    ("lock.unknown_word", "No word called {}."),
    ("notice.words", "Write these words down and keep them away from others:\n{}"),
    ("notice.words_written", "Noted. olc words shows them again when you ask."),
    ("notice.words_made", "Words made. The key they give receives at {}"),
    ("notice.words_restored", "Restored. This wallet now receives at {}"),
    ("notice.no_words", "This wallet has no words yet. Run olc words new."),
    ("notice.has_words", "This wallet already has words."),
    ("notice.words_count", "There are {} words. Put in 12, 15, 18, 21 or 24."),
    ("notice.words_unknown", "Word {} is not on the list. Check its spelling."),
    ("notice.words_checksum", "The words do not fit together. Check their order."),
    ("notice.message_signed", "{}"),
    ("notice.message_good", "The signature is right. It comes from {}"),
    ("notice.message_bad", "The signature does not match this message and address."),
    ("notice.shared_bad", "That shared wallet does not read right."),
    ("notice.shared_not_mine", "None of these keys is in this wallet."),
    ("notice.shared_made", "Shared wallet added. Give this text to the others:\n{}"),
    ("notice.shared_not_yet", "Shared wallets work from block {}."),
    ("notice.not_waiting", "That payment is not waiting."),
    ("notice.sped_up", "Sent again with a fee of {}."),
    ("notice.moved_to_words", "Moved {} to the words."),
    ("notice.shared_sent", "Sent from the shared wallet: {}"),
    ("notice.shared_waiting", "Your signature is in. Pass this to the other signers:\n{}"),
    ("notice.shared_refused", "The shared wallet could not send this."),
    ("lock.waiting", "Your signature is in. Pass this to the other signer:\n{}"),
    ("swap.accepted", "Offer taken. Pass this answer back to the one who made it:\n{}"),
    ("swap.bad_text", "That text is not a swap offer or answer."),
    ("swap.busy", "A swap with {} is still open. Finish it first."),
    ("swap.came_back", "{} came back to you."),
    ("swap.cancelled", "Swap {} cancelled."),
    ("swap.cannot_cancel", "Swap {} has coins locked already. They come back by themselves when the time is up."),
    ("swap.claimed", "Took {} from the other side. Waiting for a block."),
    ("swap.done", "Swap done. {} is yours."),
    ("swap.failed", "The swap could not start: {}"),
    ("swap.known", "This swap is here already."),
    ("swap.list", "{}"),
    ("swap.locked", "Locked {} for the swap."),
    ("swap.no_coin", "No coin called {} here. Name a coin this app ran, or a coin file."),
    ("swap.not_enough", "Not enough coins to give {} and the fee."),
    ("swap.not_saved", "The swap was not saved: {}"),
    ("swap.offered", "Offer made. Pass this text to the other side:\n{}"),
    ("swap.pay_with_main", "You pay with {}. Run that coin here first."),
    ("swap.refunding", "Time is up. Taking {} back."),
    ("swap.same_coin", "Both sides would be {}. Pick another coin."),
    ("swap.started", "The swap is on: {}. Your coins get locked now."),
    ("swap.too_late", "Their coins came too late, so {} stayed with you."),
    ("swap.unknown", "No swap matches that."),
];

/// The key a node of this app gave for its failure, when there are words for it.
pub(crate) fn known_key(key: &str) -> Option<&'static str> {
    SENTENCES.iter().map(|(known, _)| *known).find(|known| *known == key)
}

impl Notice {
    /// The notice as the command line says it: its sentence, with the detail where it belongs.
    pub fn sentence(&self) -> String {
        // What went well carries names and data, such as a coin called Test, as they are.
        let detail = if self.good { self.detail.as_str() } else { plain_detail(&self.detail) };
        let Some((_, text)) = SENTENCES.iter().find(|(key, _)| *key == self.key) else {
            return detail.to_string();
        };
        if text.contains("{}") {
            return text.replace("{}", detail);
        }
        // A failure's detail says why, such as a file that was not there.
        if self.good || detail.is_empty() || detail == *text {
            return text.to_string();
        }
        let end = if detail.ends_with(|c: char| c.is_alphanumeric()) { "." } else { "" };
        format!("{text} {detail}{end}")
    }
}

struct ConsoleDesk {
    core: Core,
    password: Option<String>,
}

impl Desk for ConsoleDesk {
    fn command(&mut self, command: Command) -> Result<String, String> {
        let core = &self.core;
        match command {
            Command::Own(Personal::Explore(text)) => Ok(looked_up(core, text)),
            command => answer_to(&|| core.snapshot(), &|one| core.send(one), command),
        }
    }

    fn boards(&mut self) -> Result<Vec<BoardView>, String> {
        Ok(self.core.snapshot().boards.clone())
    }

    fn password(&mut self) -> Result<String, String> {
        self.password.take().ok_or_else(|| "This word needs a password.".to_string())
    }
}

impl Core {
    /// Runs a typed line on its own thread, since a board may take seconds to answer.
    pub fn console(&self, line: String, password: Option<String>) -> Receiver<Result<String, String>> {
        let (answer, receiver) = channel();
        let core = self.clone();
        thread::spawn(move || {
            let result = match line.trim() {
                "help" => Ok(usage()),
                typed => run_line(&mut ConsoleDesk { core, password }, typed),
            };
            let _ = answer.send(result);
        });
        receiver
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;

    /// Every `"notice.…"` and `"lock.…"` key written in this crate's code.
    fn keys_in(folder: &Path, keys: &mut Vec<String>) {
        for entry in fs::read_dir(folder).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                keys_in(&path, keys);
                continue;
            }
            let text = fs::read_to_string(&path).unwrap();
            for start in ["\"notice.", "\"lock."] {
                for (at, _) in text.match_indices(start) {
                    let key: String = text[at + 1..].chars().take_while(|c| *c != '"').collect();
                    let rest = key.split_once('.').map_or("", |(_, rest)| rest);
                    let named = !rest.is_empty() && rest.chars().all(|c| c.is_ascii_lowercase() || c == '_');
                    if named {
                        keys.push(key);
                    }
                }
            }
        }
    }

    #[test]
    fn every_notice_the_core_gives_reads_as_a_sentence_on_the_command_line() {
        let mut keys = Vec::new();
        keys_in(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut keys);
        assert!(keys.len() > 40, "{keys:?}");
        let missing: Vec<_> = keys.iter().filter(|key| !SENTENCES.iter().any(|(had, _)| had == key)).collect();
        assert!(missing.is_empty(), "no sentence for {missing:?}");
    }

    #[test]
    fn a_failure_says_why_in_words_and_never_by_its_key() {
        let notice = |key, detail: &str, good| Notice { number: 1, key, detail: detail.into(), good };
        let failed = notice("notice.key_failed", "NoSuchKey", false).sentence();
        assert_eq!(failed, "The address did not change. This wallet has no such key.");
        let gone = notice("notice.offline_failed", "a.json: No such file", false).sentence();
        assert_eq!(gone, "That payment did not work. a.json: No such file.");
        assert_eq!(notice("notice.saved", "", true).sentence(), "Saved.");
        assert_eq!(notice("lock.bad_text", "", false).sentence(), "That text is not a spend.");
    }
}
