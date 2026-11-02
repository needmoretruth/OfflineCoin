//! What the service tells boards, and addresses a person adds.
use super::Service;
use crate::core::board_action::BoardAction;
use crate::core::board_lines::{read_state, run_action};
use crate::core::coins::{coin_folder, format_coins, parse_coins};
use crate::core::commands::{Command, Personal};
use crate::core::host::PayError;
use olc_control::BoardId;
use olc_core::CoinConfig;
use olc_node::Rank;
use olc_wallet::WalletError;
use std::thread;
use std::time::Duration;

/// A failed payment as the key of a sentence and what to put in it.
pub(super) fn pay_fault(error: &PayError) -> (&'static str, String) {
    match error {
        PayError::BadAddress => ("notice.bad_address", String::new()),
        PayError::Wallet(WalletError::NotEnoughCoins) => ("notice.not_enough", String::new()),
        PayError::Wallet(WalletError::Refused) => ("notice.pay_refused", String::new()),
        PayError::Wallet(WalletError::ZeroAmount) => ("notice.bad_amount", String::new()),
        PayError::Stopped | PayError::Wallet(_) => ("notice.pay_failed", String::new()),
    }
}

impl PayError {
    /// The failure as the key of a sentence and what to put in it.
    pub fn notice(&self) -> (&'static str, String) {
        pay_fault(self)
    }
}

/// How often a chain's loading is looked at while the app closes.
const FINISH_STEP: Duration = Duration::from_millis(20);

impl Service {
    /// Runs an order now, or keeps it for when the chain is in. Nothing lands on an engine about
    /// to be thrown away, and orders stay in the order they came.
    pub(super) fn take(&mut self, command: Command) {
        if let Command::Finish(done) = command {
            while !self.held.is_empty() {
                self.let_go();
                thread::sleep(FINISH_STEP);
            }
            let _ = done.send(());
            return;
        }
        if !command.needs_the_chain() || !(self.host.opening() || !self.held.is_empty()) {
            return self.handle(command);
        }
        if self.held.is_empty() {
            self.say("notice.held", String::new(), true);
        }
        self.held.push_back(command);
    }

    /// Runs what waited. An order that moves the node to a chain makes the rest wait for that one.
    pub(super) fn let_go(&mut self) {
        while !self.host.opening() {
            let Some(command) = self.held.pop_front() else {
                return;
            };
            self.handle(command);
        }
    }

    pub(super) fn join(&mut self, coin: CoinConfig, address: Option<String>) {
        if self.settings.coin.as_deref() == Some(coin_folder(&coin).as_str()) {
            if let Some(address) = address {
                self.settings.peers.push(address.clone());
                self.save();
                self.host.connect(address);
            }
            return;
        }
        let name = coin.name.clone();
        if self.move_to(coin, address.into_iter().collect(), None) {
            self.say("notice.joined", name, true);
        }
    }

    pub(super) fn pay(&mut self, to: &str, amount: &str) {
        let Some(units) = parse_coins(amount).filter(|units| *units > 0) else {
            self.say("notice.bad_amount", amount.to_string(), false);
            return;
        };
        let result = match self.host.pay(to, units) {
            Ok(_) => ("notice.sent", format_coins(units), true),
            Err(error) => {
                let (key, detail) = pay_fault(&error);
                (key, detail, false)
            }
        };
        self.say(result.0, result.1, result.2);
    }

    pub(super) fn add_address(&mut self, address: String) {
        let address = address.trim().to_string();
        if address.is_empty() {
            return;
        }
        let full = match address.rsplit_once(':') {
            Some((_, port)) if port.parse::<u16>().is_ok() => address,
            _ => format!("{address}:{}", crate::settings::NODE_PORT),
        };
        if !self.settings.added.contains(&full) {
            self.settings.added.push(full.clone());
        }
        self.found.added(&full);
        if self.settings.to_dial(self.host.port).contains(&full) {
            self.host.connect(full);
        }
        self.save();
        self.plan_scan();
        self.scanner.look_now();
    }

    pub(super) fn read_state(&mut self, id: BoardId) {
        if let Some(fleet) = self.fleet.clone() {
            read_state(fleet, self.told.clone(), id);
        }
    }

    /// This node takes the order at once, and so does every board in touch.
    pub(super) fn set_links(&mut self, rank: Rank) {
        let (key, detail, good) = self.host.obey(Personal::SetLinks(rank));
        self.say(key, detail, good);
        let rows = self.fleet.as_ref().map(|fleet| fleet.board_rows());
        for row in rows.unwrap_or_default().iter().filter(|row| row.seen.online) {
            self.board_action(row.seen.id, BoardAction::Links(rank.text()));
        }
    }

    pub(super) fn board_action(&mut self, id: BoardId, action: BoardAction) {
        self.returning.note_line(id, &action.line().unwrap_or_default());
        if let Some(fleet) = self.fleet.clone() {
            run_action(fleet, (self.late.clone(), self.told.clone()), id, action);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refused_payment_has_its_own_sentence() {
        assert_eq!(pay_fault(&PayError::Wallet(WalletError::Refused)).0, "notice.pay_refused");
        assert_eq!(pay_fault(&PayError::Wallet(WalletError::NotEnoughCoins)).0, "notice.not_enough");
        assert_eq!(pay_fault(&PayError::BadAddress).0, "notice.bad_address");
    }

    #[test]
    fn a_failed_payment_carries_no_name_from_the_code() {
        let errors = [
            PayError::Stopped,
            PayError::Wallet(WalletError::BadKey),
            PayError::Wallet(WalletError::NoSuchKey),
            PayError::Wallet(WalletError::ZeroAmount),
        ];
        for error in &errors {
            let (key, detail) = pay_fault(error);
            assert!(detail.is_empty(), "{key} carries {detail}");
        }
    }
}
