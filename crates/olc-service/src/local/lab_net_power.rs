//! The Lab's hashing: what each miner really hashes at under the one budget for this computer, the
//! fair attack scene, a pool that turns bad, and how each attack ended. An attack is only as
//! strong as the hashing really behind it: nothing here gives an attacker more than the speed a
//! person set.
use super::*;
use crate::local::Budget;
use olc_lab::Step;

/// The attack scene's speeds: honest miners, then the two bad miners. Out of 40,000 H/s the first
/// bad miner holds 60 % and the second 30 %, so one usually wins and the other usually loses.
pub(super) const FAIR_MINER: u64 = 2_000;
pub(super) const FAIR_ATTACKERS: [u64; 2] = [24_000, 12_000];

/// The one hash budget for this computer as the Lab feeds and reads it: the rest of OLC Core
/// counts the Lab's miners as miners like its own and splits the person's limit among them all.
pub trait Share: Send {
    /// The Lab's miners that hash now, by number, and the speed each was set to.
    fn ask(&self, asks: Vec<(u32, u64)>);
    /// The budget as last split, with a part for each Lab miner it counted.
    fn split(&self) -> Budget;
}

impl Keeper {
    /// The speeds the miners hashing were set to, together.
    fn hash_asked(&self) -> u64 {
        self.members.iter().filter(|member| member.kind.mines() && !member.paused).map(|member| member.speed).sum()
    }

    /// The Lab's turn with the one budget: it says which miners hash at what speed when that
    /// changed, and holds them to what the budget gave them together. A new miner is not in the
    /// budget until this app and its miners were lowered for it; until then it shares what the
    /// Lab had, so the sum on this computer never goes over.
    pub(super) fn share_turn(&mut self) {
        let Some(link) = &self.link else {
            return;
        };
        let asks: Vec<(u32, u64)> =
            self.members.iter().filter(|member| member.kind.mines() && !member.paused).map(|member| (member.id, member.speed)).collect();
        if asks != self.asks_told {
            link.ask(asks.clone());
            self.asks_told = asks.clone();
        }
        let split = link.split();
        // Before this computer's speed is known and with no limit there is nothing to share.
        let budget = (split.total > 0).then(|| asks.iter().filter_map(|(id, _)| split.lab_part(*id)).sum::<u64>());
        self.limit = split.limit;
        if budget != self.budget {
            self.budget = budget;
            self.limits();
        }
    }

    /// What a miner set to `speed` really hashes at: all of it, or past what the budget gives the
    /// Lab's miners together the same part as every other miner, so the shares stay what the
    /// person chose and the 51 % attack races with the share it was given. `joining` is the speed
    /// of a miner about to start, not counted yet.
    pub(super) fn limit_for(&self, speed: u64, joining: u64) -> u64 {
        let asked = self.hash_asked() + joining;
        let Some(budget) = self.budget.filter(|budget| asked > *budget) else {
            return speed;
        };
        let part = u128::from(speed) * u128::from(budget) / u128::from(asked);
        u64::try_from(part).unwrap_or(speed).max(1)
    }

    /// Every miner's real speed, told to its node when it changed. A node in the running
    /// experiment stays as the experiment holds it, and so does the one making blocks.
    pub(super) fn limits(&mut self) {
        let maker = self.runs.maker.map(|(id, _, _)| id);
        let mut tell = Vec::new();
        for at in 0..self.members.len() {
            let (id, kind, speed, busy) = {
                let member = &self.members[at];
                (member.id, member.kind, member.speed, member.busy)
            };
            if !kind.mines() {
                continue;
            }
            let limit = self.limit_for(speed, 0);
            self.members[at].limit = limit;
            if !busy && Some(id) != maker {
                tell.push((id, limit));
            }
        }
        for (id, limit) in tell {
            self.tell(id, "limit", format!("board limit {limit}"));
        }
    }

    /// The miner making blocks hashes with all the Lab's part leaves it, or uncapped without one.
    pub(super) fn maker_limit(&self, id: u32) -> u64 {
        let Some(budget) = self.budget else {
            return 0;
        };
        let others: u64 = self.members.iter().filter(|member| member.kind.mines() && !member.paused && member.id != id).map(|member| member.limit).sum();
        budget.saturating_sub(others).max(1)
    }

    /// A new participant's speed: a miner's modest middle preset, the same for a bad miner.
    pub(super) fn start_speed(kind: Kind) -> u64 {
        kind.speed().map_or(0, |range| range.presets[1])
    }

    /// The fair attack: two honest miners and two bad miners at the scene's speeds.
    pub(super) fn fair_speeds(&mut self, miners: &[u32], attackers: &[u32]) {
        let planned: Vec<(u32, u64)> = miners.iter().map(|id| (*id, FAIR_MINER)).chain(attackers.iter().copied().zip(FAIR_ATTACKERS)).collect();
        for (id, speed) in planned {
            if let Some(member) = self.member_mut(id) {
                member.speed = speed;
            }
        }
        self.limits();
    }

    /// A pool turns bad: it holds what it owes its miners and attacks with their power once it
    /// has the coins. Turned honest, it pays them again and attacks no more.
    pub(super) fn turn_bad(&mut self, id: u32, bad: bool) -> Result<(), String> {
        let member = self.member(id).ok_or("net.gone")?;
        if member.kind != Kind::Pool {
            return Ok(());
        }
        if member.busy {
            return Err("net.busy".into());
        }
        if let Some(member) = self.member_mut(id) {
            member.bad = bad;
            member.raced = false;
        }
        self.send(id, if bad { "lab pool hold on" } else { "lab pool hold off" });
        Ok(())
    }

    /// The miners whose work goes to a pool, the bad pool's power.
    pub(super) fn crew_of(&self, pool: u32) -> Vec<u32> {
        self.members.iter().filter(|other| other.pool == Some(pool) && !other.paused && other.status.is_some()).map(|other| other.id).collect()
    }

    /// The end of an attack as one event: who raced, with what share of all hashing, and how.
    /// A race counts as won only once the victim moved to the hidden chain.
    pub(super) fn tell_race(&mut self, by: u32) {
        let Some(run) = &self.runs.experiment else {
            return;
        };
        let (mut share, mut end) = (None, None);
        for frame in &run.frames {
            match frame.step {
                Step::Power { attacker, victim } => share = Some((attacker * 100).checked_div(attacker + victim).unwrap_or(0)),
                Step::Replaced { hidden_won: true, .. } => end = Some((true, 0)),
                Step::GaveUp { behind } => end = Some((false, behind)),
                _ => {}
            }
        }
        if let (Some(share), Some((won, behind))) = (share, end) {
            self.event(What::Raced { by, won, share, behind });
        }
    }
}
