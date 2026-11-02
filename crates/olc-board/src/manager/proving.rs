//! A board on power alone takes only a build a board with a cable has proved. The app reaches
//! every cabled board by its cable, so it makes that proof itself: the build goes to a cabled
//! board by cable, then once more with that cable set aside, so the board is reached through
//! another board by radio and must come back from it.
use super::{hex, Manager};
use crate::clearance::Ledger;
use crate::Connection;
use olc_control::BoardId;
use olc_crypto::Hash;
use std::thread;
use std::time::{Duration, Instant};

/// No cabled board could try the build by radio. The board's page says what to do.
pub const NO_PROVER: &str = "No board with a cable could try this build by radio.";
/// How long a cabled board may take to be heard through another board once its cable is aside.
const REACH_BY_RADIO: Duration = Duration::from_secs(60);

/// What has to happen before a board may take a build.
#[derive(Debug, PartialEq, Eq)]
pub enum Proof {
    /// The board has a cable, or the build is proved already.
    NotNeeded,
    /// Deliver by cable first when the cabled board has not taken this build yet, then by radio.
    On { board: BoardId, by_cable_first: bool },
    Impossible,
}

/// Chooses how a build gets its proof. `cabled` are the cabled boards heard on their cables now,
/// each with whether the last build it took is this one.
pub fn proof_for(target_cabled: bool, proven: bool, cabled: &[(BoardId, bool)]) -> Proof {
    if target_cabled || proven {
        return Proof::NotNeeded;
    }
    // One that already took this build needs only the delivery by radio.
    let ready = cabled.iter().find(|(_, ran)| *ran).or_else(|| cabled.first());
    ready.map_or(Proof::Impossible, |(board, ran)| Proof::On { board: *board, by_cable_first: !ran })
}

impl Manager {
    /// Proves the build for `target` when it needs it, saying each stage on its page.
    pub(super) fn prove_for(&self, target: BoardId, hash: &Hash, ledger: &Ledger) -> Result<(), String> {
        let cabled = self.cabled_boards(target, hash, ledger);
        let proof = proof_for(ledger.has_cable(target), ledger.is_proven(hash), &cabled);
        let (prover, by_cable_first) = match proof {
            Proof::NotNeeded => return Ok(()),
            Proof::Impossible => return Err(NO_PROVER.to_string()),
            Proof::On { board, by_cable_first } => (board, by_cable_first),
        };
        let key = hex(&target);
        let name = self.name_of(prover);
        if by_cable_first {
            self.set_job(&key, Some(format!("proof_cable {name}")));
            if !self.update(prover) {
                return Err("The board with a cable did not take the build.".to_string());
            }
        }
        self.set_job(&key, Some(format!("proof_radio {name}")));
        let Some(path) = self.path_of(prover) else {
            return Err(NO_PROVER.to_string());
        };
        if let Ok(mut busy) = self.busy_ports.lock() {
            busy.insert(path.clone());
        }
        let proved = self.reached_by_radio(prover) && self.update(prover) && ledger.is_proven(hash);
        if let Ok(mut busy) = self.busy_ports.lock() {
            busy.remove(&path);
        }
        if proved {
            Ok(())
        } else {
            Err(NO_PROVER.to_string())
        }
    }

    /// Cabled boards other than `target`, heard on their own cables just now.
    fn cabled_boards(&self, target: BoardId, hash: &Hash, ledger: &Ledger) -> Vec<(BoardId, bool)> {
        let boards = self.fleet.lock().map(|fleet| fleet.boards()).unwrap_or_default();
        let on_cable = |link: usize| self.ports.lock().is_ok_and(|ports| ports.values().any(|port| port.link == link));
        boards
            .iter()
            .filter(|seen| seen.id != target && seen.online && ledger.has_cable(seen.id))
            .filter(|seen| matches!(seen.connection, Connection::Direct { link } if on_cable(link)))
            .map(|seen| (seen.id, ledger.last_ran(seen.id, hash)))
            .collect()
    }

    fn name_of(&self, board: BoardId) -> String {
        let boards = self.fleet.lock().map(|fleet| fleet.boards()).unwrap_or_default();
        let found = boards.into_iter().find(|seen| seen.id == board);
        found.map_or_else(|| hex(&board), |seen| seen.beacon.name)
    }

    /// The port a board is heard on now.
    fn path_of(&self, board: BoardId) -> Option<String> {
        let boards = self.fleet.lock().ok()?.boards();
        let Connection::Direct { link } = boards.iter().find(|seen| seen.id == board)?.connection else {
            return None;
        };
        let ports = self.ports.lock().ok()?;
        ports.iter().find(|(_, port)| port.link == link).map(|(path, _)| path.clone())
    }

    /// Waits until the board, its cable set aside, is heard through another board.
    fn reached_by_radio(&self, board: BoardId) -> bool {
        let deadline = Instant::now() + REACH_BY_RADIO;
        while Instant::now() < deadline {
            let boards = self.fleet.lock().map(|fleet| fleet.boards()).unwrap_or_default();
            let through = boards.iter().any(|seen| {
                seen.id == board
                    && seen.silent_for < Duration::from_secs(3)
                    && matches!(seen.connection, Connection::Through { .. })
            });
            if through {
                return true;
            }
            thread::sleep(Duration::from_millis(500));
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ONE: BoardId = [1; 6];
    const TWO: BoardId = [2; 6];

    #[test]
    fn a_cabled_board_or_a_proved_build_needs_no_proof() {
        assert_eq!(proof_for(true, false, &[]), Proof::NotNeeded);
        assert_eq!(proof_for(false, true, &[]), Proof::NotNeeded);
    }

    #[test]
    fn with_no_cabled_board_the_proof_cannot_happen() {
        assert_eq!(proof_for(false, false, &[]), Proof::Impossible);
    }

    #[test]
    fn a_new_build_goes_by_cable_then_by_radio() {
        let proof = proof_for(false, false, &[(ONE, false)]);
        assert_eq!(proof, Proof::On { board: ONE, by_cable_first: true });
    }

    #[test]
    fn a_board_that_already_took_the_build_needs_only_the_radio() {
        let proof = proof_for(false, false, &[(ONE, false), (TWO, true)]);
        assert_eq!(proof, Proof::On { board: TWO, by_cable_first: false });
    }

    fn manager_in(name: &str) -> Manager {
        use crate::control_key::ControlKey;
        use crate::fleet::Fleet;
        use std::sync::{Arc, Mutex};
        let folder = std::env::temp_dir().join(format!("olc-proving-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let key = ControlKey::load_or_create(&folder).unwrap();
        let fleet = Fleet::new(key, olc_node::Runner::link_of).unwrap();
        Manager {
            fleet: Arc::new(Mutex::new(fleet)),
            firmware: std::path::PathBuf::new(),
            fixed_ports: Vec::new(),
            ports: Mutex::default(),
            busy_ports: Mutex::default(),
            loader_wakes: Mutex::default(),
            jobs: Mutex::default(),
            run: Arc::default(),
        }
    }

    /// During the proof the cabled board's port is let go, and a person must not be offered to
    /// install a new board on it, while a port really being installed stays in view.
    #[test]
    fn a_port_set_aside_for_the_proof_is_no_new_board() {
        let manager = manager_in("aside");
        let (aside, installing) = ("port-one", "port-two");
        manager.busy_ports.lock().unwrap().insert(aside.into());
        manager.set_job(&hex(&ONE), Some("proof_radio one".into()));
        assert!(manager.new_boards().is_empty(), "the set-aside port shows as a new board");
        manager.busy_ports.lock().unwrap().insert(installing.into());
        manager.set_job(installing, Some("installing".into()));
        assert_eq!(manager.new_boards(), vec![installing.to_string()]);
    }
}
