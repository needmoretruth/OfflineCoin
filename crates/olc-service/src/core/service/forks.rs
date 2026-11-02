//! Changing the running coin's rules. The genesis and history stay, this node and its local nodes
//! carry on in a folder of their own, and the coin they left stays whole to go back to.
use super::{current_coin, Service};
use crate::core::coins::{coin_folder, copy_chain};
use olc_store::coin_fields::{first_fork_height, fork_coin};

impl Service {
    pub(super) fn fork(&mut self, at: Option<u64>, changes: &[(String, String)]) {
        let coin = current_coin(&self.paths, &self.settings);
        let tip = self.host.view().height;
        let from_height = at.unwrap_or_else(|| first_fork_height(&coin, tip));
        match fork_coin(&coin, from_height, tip, changes) {
            Ok(forked) => {
                let peers = self.settings.peers.clone();
                if self.move_to(forked, peers, Some(coin_folder(&coin))) {
                    self.say("notice.forked", from_height.to_string(), true);
                }
            }
            Err(bad) => self.say("notice.fork_refused", bad.to_string(), false),
        }
    }

    /// The folder of the earlier rules stayed whole, so going back is joining it again.
    pub(super) fn fork_back(&mut self) {
        let coin = current_coin(&self.paths, &self.settings);
        let Some(before) = coin.before_last_fork() else {
            let why = "This coin has no earlier rules.".to_string();
            return self.say("notice.fork_refused", why, false);
        };
        let (name, peers) = (before.name.clone(), self.settings.peers.clone());
        if self.move_to(before, peers, None) {
            self.say("notice.forked_back", name, true);
        }
    }

    /// Each local node keeps its own chain, so each carries it over by itself.
    pub(super) fn carry_local_chains(&mut self, old: &str, new: &str) {
        for spec in self.settings.nodes.clone() {
            let folder = self.paths.local_node(spec.id);
            if let Err(error) = copy_chain(&folder.join(old), &folder.join(new)) {
                self.say("notice.node_failed", error, false);
            }
        }
    }
}
