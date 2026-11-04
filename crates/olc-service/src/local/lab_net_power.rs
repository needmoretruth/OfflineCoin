//! The Lab's hashing: what each miner really hashes at under the person's limit, the fair attack
//! scene, a pool that turns bad, and how each attack ended. An attack is only as strong as the
//! hashing really behind it: nothing here gives an attacker more than the speed a person set.
use super::*;
use olc_lab::Step;

/// The attack scene's speeds: honest miners, then the two bad miners. Out of 40,000 H/s the first
/// bad miner holds 60 % and the second 30 %, so one usually wins and the other usually loses.
pub(super) const FAIR_MINER: u64 = 2_000;
pub(super) const FAIR_ATTACKERS: [u64; 2] = [24_000, 12_000];

impl Keeper {
    /// The speeds the miners hashing were set to, together.
    fn hash_asked(&self) -> u64 {
        self.members.iter().filter(|member| member.kind.mines() && !member.paused).map(|member| member.speed).sum()
    }

    /// What a miner set to `speed` really hashes at: all of it, or under the person's limit the
    /// same part as every other miner, so the shares stay what the person chose. `joining` is
    /// the speed of a miner about to start, not counted yet.
    pub(super) fn limit_for(&self, speed: u64, joining: u64) -> u64 {
        let asked = self.hash_asked() + joining;
        if self.budget == 0 || asked <= self.budget {
            return speed;
        }
        let part = u128::from(speed) * u128::from(self.budget) / u128::from(asked);
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

    /// The miner making blocks hashes with all the limit leaves it, or uncapped with no limit.
    pub(super) fn maker_limit(&self, id: u32) -> u64 {
        if self.budget == 0 {
            return 0;
        }
        let others: u64 = self.members.iter().filter(|member| member.kind.mines() && !member.paused && member.id != id).map(|member| member.limit).sum();
        self.budget.saturating_sub(others).max(1)
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
