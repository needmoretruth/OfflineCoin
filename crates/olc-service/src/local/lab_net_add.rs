//! Adding, changing and removing participants, and the one-click setups.
use super::*;
use crate::local::supervisor::address_of;

/// The share a pool asks of its miners: at the usual speed a miner hands in about two a second.
pub const SHARE_DIFFICULTY: u64 = 4_000;

impl Keeper {
    pub(super) fn add(&mut self, kind: Kind) -> Result<u32, String> {
        if Room::measure(self.members.len()).full() {
            return Err("net.room_full".into());
        }
        if self.supervisor.is_none() {
            let (coin, config, supervisor) = fresh_place(self.sweep)?;
            self.sweep = false;
            self.coin = Some(coin);
            self.config = Some(config);
            self.supervisor = Some(supervisor);
        }
        let number = {
            let count = self.counts.entry(kind).or_insert(0);
            *count += 1;
            *count
        };
        let id = self.next_id;
        self.next_id += 1;
        // A bad miner starts as any miner does: its power is what a person gives it.
        let speed = Self::start_speed(kind);
        let limit = if kind.mines() { self.limit_for(speed, speed) } else { 0 };
        let spec = spec_of(id, kind, number, limit);
        let links = self.dial_for();
        let peers: Vec<String> = links.iter().filter_map(|id| self.address_of_member(*id)).collect();
        let coin = self.coin.clone().ok_or("net.failed")?;
        let supervisor = self.supervisor.as_mut().ok_or("net.failed")?;
        supervisor.start(&spec, &coin, &peers)?;
        // A board line turns on the chain in the status line, a lab line the network's part.
        supervisor.send(id, "board status")?;
        supervisor.send(id, "lab on")?;
        if kind == Kind::Pool {
            supervisor.send(id, &format!("lab pool run {SHARE_DIFFICULTY}"))?;
        }
        let pid = supervisor.pid(id);
        self.specs.insert(id, spec);
        self.members.push(Member {
            id,
            kind,
            number,
            speed,
            limit,
            paused: false,
            pool: None,
            links,
            status: None,
            pid,
            ended: None,
            waits: None,
            tricks: Vec::new(),
            received: Received::default(),
            busy: false,
            bad: false,
            raced: false,
        });
        Ok(id)
    }

    /// The first participant, which everyone dials, and the newest one, so the map stays a web.
    fn dial_for(&self) -> Vec<u32> {
        let mut links: Vec<u32> = self.members.first().map(|member| member.id).into_iter().collect();
        if let Some(last) = self.members.last() {
            if !links.contains(&last.id) {
                links.push(last.id);
            }
        }
        links
    }

    /// Where a participant listens: the port it took, or the one it was asked to take.
    pub(super) fn address_of_member(&self, id: u32) -> Option<String> {
        let member = self.member(id)?;
        match member.port() {
            Some(port) => Some(format!("127.0.0.1:{port}")),
            None => self.specs.get(&id).map(address_of),
        }
    }

    pub(super) fn remove(&mut self, id: u32) {
        if self.runs.uses(id) {
            self.stop_run();
        }
        if let Some(supervisor) = &mut self.supervisor {
            supervisor.stop(id);
        }
        self.specs.remove(&id);
        self.members.retain(|member| member.id != id);
        for member in &mut self.members {
            if member.pool == Some(id) {
                member.pool = None;
            }
            member.links.retain(|link| *link != id);
        }
        self.told.retain(|(member, _), _| *member != id);
        if self.members.is_empty() {
            self.clear();
        }
    }

    pub(super) fn set_speed(&mut self, id: u32, speed: u64) -> Result<(), String> {
        let member = self.member(id).ok_or("net.gone")?;
        if member.busy {
            return Err("net.busy".into());
        }
        let range = member.kind.speed().ok_or("net.no_speed")?;
        let speed = speed.clamp(range.least, range.most);
        let (kind, paused) = (member.kind, member.paused);
        if let Some(member) = self.member_mut(id) {
            member.speed = speed;
        }
        if paused {
            // A speed chosen after a stop starts it again at that speed.
            return self.pause(id, false);
        }
        if kind.mines() {
            self.limits();
        }
        Ok(())
    }

    /// A miner stops hashing, a sender or spammer stops paying, and any other keeps to itself.
    pub(super) fn pause(&mut self, id: u32, paused: bool) -> Result<(), String> {
        let member = self.member(id).ok_or("net.gone")?;
        if member.busy {
            return Err("net.busy".into());
        }
        let kind = member.kind;
        if let Some(member) = self.member_mut(id) {
            member.paused = paused;
        }
        // One stopping or starting changes what the others may use under the person's limit.
        self.limits();
        match kind {
            Kind::Miner | Kind::Attacker if paused => self.send(id, "board mine off"),
            Kind::Miner | Kind::Attacker => self.send(id, "board mine on"),
            // The spammer's rate is told again on the next turn.
            Kind::Sender | Kind::Spammer => {}
            _ => self.send(id, if paused { "board offline on" } else { "board offline off" }),
        }
        Ok(())
    }

    pub(super) fn again(&mut self, id: u32) -> Result<(), String> {
        let member = self.member(id).ok_or("net.gone")?;
        match member.kind {
            Kind::Attacker | Kind::Pool => {
                if self.runs.running() {
                    return Err("net.busy".into());
                }
                if let Some(member) = self.member_mut(id) {
                    member.raced = false;
                }
            }
            Kind::Exploiter => {
                self.tried.remove(&id);
                if let Some(member) = self.member_mut(id) {
                    member.tricks.clear();
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Every process stops and the network's folder goes; the next participant starts a new coin.
    pub(super) fn clear(&mut self) {
        self.stop_run();
        if let Some(mut supervisor) = self.supervisor.take() {
            supervisor.stop_all();
        }
        forget_folder(self.coin.take());
        let next_id = self.next_id;
        let (shared, room, sweep) = (self.shared.clone(), self.room, self.sweep);
        *self = Keeper::new(shared);
        self.next_id = next_id;
        self.room = room;
        self.sweep = sweep;
    }

    pub(super) fn scene(&mut self, scene: Scene) -> Result<(), String> {
        self.clear();
        let mut added = Vec::new();
        for kind in scene.cast() {
            added.push((*kind, self.add(*kind)?));
        }
        let miners: Vec<u32> = added.iter().filter(|(kind, _)| *kind == Kind::Miner).map(|(_, id)| *id).collect();
        match scene {
            Scene::Pool => {
                let pool = added.iter().find(|(kind, _)| *kind == Kind::Pool).map(|(_, id)| *id);
                for id in miners.iter().take(2) {
                    if let Some(member) = self.member_mut(*id) {
                        member.pool = pool;
                    }
                }
            }
            Scene::Split => self.split_asked = true,
            Scene::Attack => {
                let attackers: Vec<u32> = added.iter().filter(|(kind, _)| *kind == Kind::Attacker).map(|(_, id)| *id).collect();
                self.fair_speeds(&miners, &attackers);
            }
            _ => {}
        }
        self.scene = Some(scene);
        Ok(())
    }

    /// The first miner mines as fast as it can, or as the person's limit leaves it, until the
    /// chain is `count` blocks longer.
    pub(super) fn make_blocks(&mut self, count: u64) -> Result<(), String> {
        if self.runs.running() || self.runs.making.is_some() {
            return Err("net.busy".into());
        }
        let miner = self.members.iter().find(|member| member.kind == Kind::Miner).ok_or("net.need_miner")?;
        let (id, from) = (miner.id, miner.height());
        let count = count.clamp(1, super::super::lab::MOST_BLOCKS);
        self.send(id, &format!("board mine-to {}", from + count));
        self.send(id, &format!("board limit {}", self.maker_limit(id)));
        self.send(id, "board mine on");
        self.runs.making = Some((0, count));
        self.runs.maker = Some((id, from, Instant::now()));
        Ok(())
    }
}
