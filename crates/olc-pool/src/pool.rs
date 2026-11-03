//! The pool: it hands out a block paying the pool, checks every share by hashing it, keeps who did
//! what, and sends a block that reaches the chain's difficulty to the node. Connections speak to it
//! through [`crate::session`]; the node's side is in [`crate::found`].
use crate::ledger::{Ledger, Payout, Split};
use crate::stratum::{Notify, Refusal, EXTRANONCE1_BYTES, EXTRANONCE2_BYTES};
use crate::vardiff::{rounded, Vardiff};
use olc_core::bits::{bits_of, hash_meets, target_of};
use olc_core::{Address, Block, BlockHeader, Hash, EXTRANONCE_BYTES};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// A template is built again this often, to take in new payments and move its time.
pub const REBUILD_MS: u64 = 20_000;
/// Jobs kept, so a share found just before a rebuild still counts.
const MOST_JOBS: usize = 6;
/// Shares remembered per job, to know a second copy.
const MOST_SEEN: usize = 200_000;
const SPEED_WINDOW_MS: u64 = 120_000;
/// How far past now a miner may move a block's time: the chain refuses more than two minutes.
const AHEAD_SECS: u32 = 60;
pub(crate) const MOST_BLOCKS_KEPT: usize = 100;

/// What the person running the pool chose.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Plan {
    pub payout: Payout,
    /// Hundredths of a percent the pool keeps, at most 5 %.
    pub fee_bps: u32,
    /// One share difficulty for every worker, in place of each one's own.
    pub fixed_difficulty: Option<u64>,
}

pub(crate) struct Job {
    pub(crate) number: u64,
    pub(crate) notify: Notify,
    pub(crate) block: Block,
    seen: BTreeSet<([u8; EXTRANONCE_BYTES], u32, u32, u32)>,
}

pub(crate) struct Session {
    pub(crate) extranonce1: [u8; EXTRANONCE1_BYTES],
    pub(crate) subscribed: bool,
    pub(crate) names: BTreeMap<String, Address>,
    pub(crate) vardiff: Vardiff,
    pub(crate) rolling: u32,
    pub(crate) sent_job: Option<u64>,
    pub(crate) sent_difficulty: Option<u64>,
    pub(crate) malformed: u32,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Stats {
    pub(crate) address: Address,
    pub(crate) accepted: u64,
    pub(crate) refused: u64,
    pub(crate) paid: u64,
    pub(crate) last_share_ms: Option<u64>,
    pub(crate) first_ms: u64,
    pub(crate) recent: VecDeque<(u64, u64)>,
    pub(crate) sessions: u32,
    pub(crate) difficulty: u64,
}

/// One worker as a person reads it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WorkerView {
    /// As the worker signed in: `address.name`.
    pub name: String,
    pub address: Address,
    /// Hashes a second over the last two minutes, counted from its shares.
    pub speed: u64,
    pub last_share_ms: Option<u64>,
    pub accepted: u64,
    pub refused: u64,
    pub paid: u64,
    pub difficulty: u64,
    pub online: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockState {
    /// Sent to the node, not yet in its chain.
    Sent,
    /// In the chain, its coins not yet spendable.
    Waiting,
    Paid,
    /// It left the best chain, or the node did not take it.
    Lost,
}

pub(crate) struct Found {
    pub(crate) height: u64,
    pub(crate) id: Hash,
    pub(crate) value: u64,
    pub(crate) found_ms: u64,
    pub(crate) state: BlockState,
    pub(crate) credits: crate::ledger::Credits,
    pub(crate) split: Option<Split>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockView {
    pub height: u64,
    pub id: Hash,
    pub value: u64,
    pub found_ms: u64,
    pub state: BlockState,
    /// The pool's fee, once paid.
    pub pool_fee: u64,
}

pub struct Pool {
    pub(crate) payout: Address,
    pub(crate) plan: Plan,
    pub(crate) ledger: Ledger,
    pub(crate) jobs: VecDeque<Job>,
    next_job: u64,
    tip: Hash,
    built_ms: u64,
    pub(crate) sessions: BTreeMap<u64, Session>,
    pub(crate) next_session: u64,
    pub(crate) workers: BTreeMap<String, Stats>,
    pub(crate) found: Vec<Found>,
    pub(crate) outbox: Vec<Block>,
    /// The parent of the last block the pool sealed: one more on it would only race the first.
    pub(crate) sealed_on: Option<Hash>,
}

/// True when `header`'s hash meets `difficulty`, read as Bitcoin Core's `CheckProofOfWork` reads it.
pub fn meets(header: &BlockHeader, difficulty: u64) -> bool {
    target_of(bits_of(difficulty)).is_some_and(|target| hash_meets(&header.id(), &target))
}

/// A share as it came in: which job, the miner's part of the extranonce, its time, nonce and version.
pub struct Submitted<'a> {
    pub job: &'a str,
    pub extranonce2: &'a [u8],
    pub ntime: u32,
    pub nonce: u32,
    /// The version bits a miner rolled, under the mask it agreed.
    pub version_bits: Option<u32>,
}

impl Pool {
    /// `payout` is the pool's own address, which must be a key of the node it runs on.
    pub fn new(payout: Address, plan: Plan) -> Self {
        Self {
            payout,
            plan,
            ledger: Ledger::new(plan.payout),
            jobs: VecDeque::new(),
            next_job: 1,
            tip: [0; 32],
            built_ms: 0,
            sessions: BTreeMap::new(),
            next_session: 0,
            workers: BTreeMap::new(),
            found: Vec::new(),
            outbox: Vec::new(),
            sealed_on: None,
        }
    }

    pub fn plan(&self) -> Plan {
        self.plan
    }

    pub fn payout_address(&self) -> Address {
        self.payout
    }

    /// True when the chain moved or the newest job is old enough to build again.
    pub fn wants_template(&self, tip: Hash, now_ms: u64) -> bool {
        self.jobs.is_empty() || tip != self.tip || now_ms >= self.built_ms + REBUILD_MS
    }

    /// Hands out `block`, a template paying the pool. A new tip makes every older job stale.
    /// False for a block in OLC's first header, which Stratum cannot carry.
    pub fn take_template(&mut self, block: Block, now_ms: u64) -> bool {
        let clean = block.header.previous != self.tip || self.jobs.is_empty();
        let number = self.next_job;
        let Some(notify) = Notify::of_block(format!("{number:x}"), &block, clean) else {
            return false;
        };
        self.next_job += 1;
        if clean {
            self.jobs.clear();
        }
        self.tip = block.header.previous;
        self.built_ms = now_ms;
        self.jobs.push_back(Job { number, notify, block, seen: BTreeSet::new() });
        while self.jobs.len() > MOST_JOBS {
            self.jobs.pop_front();
        }
        true
    }

    pub(crate) fn newest(&self) -> Option<&Job> {
        self.jobs.back()
    }

    /// The block's own difficulty, the most a share may ask: past it a share could hide a block.
    pub(crate) fn block_difficulty(&self) -> u64 {
        self.newest().map_or(u64::MAX, |job| job.block.header.difficulty)
    }

    /// The difficulty a new connection starts at, before its first shares say how fast it is.
    pub(crate) fn first_difficulty(&self) -> u64 {
        let wanted = self.plan.fixed_difficulty.unwrap_or(self.block_difficulty() / 16);
        rounded(wanted.clamp(1, self.block_difficulty()))
    }

    fn refuse(&mut self, session: u64, worker: Option<&str>, refusal: Refusal) -> Result<(), Refusal> {
        if let Some(stats) = worker.and_then(|worker| self.workers.get_mut(worker)) {
            stats.refused += 1;
        } else if let Some(session) = self.sessions.get_mut(&session) {
            session.malformed += 1;
        }
        Err(refusal)
    }

    /// Checks one share from `worker` on connection `session` by hashing it.
    pub fn submit(&mut self, session: u64, worker: &str, share: &Submitted, now_ms: u64) -> Result<(), Refusal> {
        let Some(open) = self.sessions.get(&session) else {
            return Err(Refusal::NotSubscribed);
        };
        if !open.subscribed {
            return self.refuse(session, None, Refusal::NotSubscribed);
        }
        if !open.names.contains_key(worker) {
            return self.refuse(session, None, Refusal::Unauthorized);
        }
        let (extranonce1, rolling) = (open.extranonce1, open.rolling);
        let difficulties = (open.vardiff.difficulty(), open.vardiff.before(now_ms));
        let Some(job) = self.jobs.iter().find(|job| job.notify.job_id == share.job) else {
            return self.refuse(session, Some(worker), Refusal::Stale);
        };
        if share.extranonce2.len() != EXTRANONCE2_BYTES {
            return self.refuse(session, Some(worker), Refusal::Malformed);
        }
        let now_secs = u32::try_from(now_ms / 1_000).unwrap_or(u32::MAX);
        let latest = job.notify.ntime.max(now_secs).saturating_add(AHEAD_SECS);
        if share.ntime < job.notify.ntime || share.ntime > latest {
            return self.refuse(session, Some(worker), Refusal::Time);
        }
        let base = job.notify.version;
        let version = match share.version_bits {
            None => base,
            Some(bits) if bits & !rolling == 0 || bits & !rolling == base & !rolling => (base & !rolling) | (bits & rolling),
            Some(_) => return self.refuse(session, Some(worker), Refusal::Version),
        };
        let mut extranonce = [0u8; EXTRANONCE_BYTES];
        extranonce[..EXTRANONCE1_BYTES].copy_from_slice(&extranonce1);
        extranonce[EXTRANONCE1_BYTES..].copy_from_slice(share.extranonce2);
        let key = (extranonce, share.ntime, share.nonce, version);
        if job.seen.contains(&key) {
            return self.refuse(session, Some(worker), Refusal::Duplicate);
        }
        if job.seen.len() >= MOST_SEEN {
            return self.refuse(session, Some(worker), Refusal::Other);
        }
        let Some(header) = job.notify.header(&extranonce, share.ntime, share.nonce, version) else {
            return self.refuse(session, Some(worker), Refusal::Malformed);
        };
        let credited = match difficulties {
            (now, _) if meets(&header, now) => now,
            (_, Some(before)) if meets(&header, before) => before,
            _ => return self.refuse(session, Some(worker), Refusal::Weak),
        };
        let number = job.number;
        // A share that would seal a second block on the parent of one already sent still counts as
        // work, as it was; sending it too would only make the pool race itself.
        let sealed = meets(&header, header.difficulty) && self.sealed_on != Some(header.previous);
        if let Some(job) = self.jobs.iter_mut().find(|job| job.number == number) {
            job.seen.insert(key);
        }
        self.count(session, worker, credited, now_ms);
        if sealed {
            self.sealed(number, &extranonce, header, now_ms);
        }
        Ok(())
    }

    fn count(&mut self, session: u64, worker: &str, credited: u64, now_ms: u64) {
        self.ledger.add(worker, credited);
        if let Some(open) = self.sessions.get_mut(&session) {
            open.vardiff.shared();
        }
        if let Some(stats) = self.workers.get_mut(worker) {
            stats.accepted += 1;
            stats.last_share_ms = Some(now_ms);
            stats.recent.push_back((now_ms, credited));
            while stats.recent.front().is_some_and(|(at, _)| at + SPEED_WINDOW_MS < now_ms) {
                stats.recent.pop_front();
            }
        }
    }

    /// A share that also meets the block's difficulty: the block goes to the node, and the work
    /// so far is what it pays for.
    fn sealed(&mut self, number: u64, extranonce: &[u8; EXTRANONCE_BYTES], header: BlockHeader, now_ms: u64) {
        let Some(job) = self.jobs.iter().find(|job| job.number == number) else {
            return;
        };
        let mut block = job.block.clone();
        let Some(last) = block.txs.first_mut().and_then(|coinbase| coinbase.outputs.last_mut()) else {
            return;
        };
        last.lock = extranonce.to_vec();
        block.header = header;
        if olc_core::types::merkle_root(&block.txs) != header.merkle_root {
            return;
        }
        let value = block.txs[0].outputs.iter().map(|output| output.value).fold(0, u64::saturating_add);
        let height = block.txs[0].coinbase_height.unwrap_or_default();
        self.found.push(Found {
            height,
            id: block.id(),
            value,
            found_ms: now_ms,
            state: BlockState::Sent,
            credits: self.ledger.close_round(),
            split: None,
        });
        self.sealed_on = Some(block.header.previous);
        self.outbox.push(block);
        while self.found.len() > MOST_BLOCKS_KEPT {
            let at = self.found.iter().position(|found| matches!(found.state, BlockState::Paid | BlockState::Lost));
            match at {
                Some(at) => drop(self.found.remove(at)),
                None => break,
            }
        }
    }

    /// Every worker that ever signed in, newest share first.
    pub fn workers(&self, now_ms: u64) -> Vec<WorkerView> {
        let mut rows: Vec<WorkerView> = self
            .workers
            .iter()
            .map(|(name, stats)| {
                let recent = stats.recent.iter().filter(|(at, _)| at + SPEED_WINDOW_MS >= now_ms);
                let work: u128 = recent.map(|(_, difficulty)| u128::from(*difficulty)).sum();
                // A worker that just came is measured over the time it has been here.
                let span = now_ms.saturating_sub(stats.first_ms).clamp(10_000, SPEED_WINDOW_MS);
                WorkerView {
                    name: name.clone(),
                    address: stats.address,
                    speed: (work * 1_000 / u128::from(span)) as u64,
                    last_share_ms: stats.last_share_ms,
                    accepted: stats.accepted,
                    refused: stats.refused,
                    paid: stats.paid,
                    difficulty: stats.difficulty,
                    online: stats.sessions > 0,
                }
            })
            .collect();
        rows.sort_by(|a, b| b.online.cmp(&a.online).then(b.last_share_ms.cmp(&a.last_share_ms)).then(a.name.cmp(&b.name)));
        rows
    }

    /// The blocks the pool found, newest first.
    pub fn blocks(&self) -> Vec<BlockView> {
        let rows = self.found.iter().rev().map(|found| BlockView {
            height: found.height,
            id: found.id,
            value: found.value,
            found_ms: found.found_ms,
            state: found.state,
            pool_fee: found.split.as_ref().map_or(0, |split| split.pool_fee),
        });
        rows.collect()
    }

    /// Work counted so far toward the next block, by worker.
    pub fn pending(&self) -> crate::ledger::Credits {
        self.ledger.pending()
    }
}
