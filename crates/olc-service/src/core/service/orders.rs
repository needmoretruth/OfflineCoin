//! What the service tells boards, and addresses a person adds.
use super::Service;
use crate::core::board_action::BoardAction;
use crate::core::board_lines::{read_bright, run_action};
use crate::core::commands::Personal;
use olc_control::BoardId;
use olc_node::Rank;

impl Service {
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
        self.save();
        self.plan_scan();
        self.scanner.look_now();
    }

    pub(super) fn read_bright(&mut self, id: BoardId) {
        if let Some(fleet) = self.fleet.clone() {
            read_bright(fleet, self.brights.clone(), id);
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
            run_action(fleet, (self.late.clone(), self.brights.clone()), id, action);
        }
    }
}
