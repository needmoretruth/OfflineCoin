//! Names the miner of every block from what this computer knows: its own wallet, the miners it
//! runs, the boards it hears. Then counts each one's share, keeps this computer's speed over ten
//! minutes and works out when the next block should come.
use super::chain_view::{ChainView, MinedBlock, Miner, Share};
use super::snapshot::{BoardView, NodeView};
use crate::local::LocalView;
use olc_core::Hash;
use olc_wallet::format_address;
use std::collections::{BTreeMap, VecDeque};

/// How far back the speed line reaches.
pub const TRAIL_MS: u64 = 600_000;
/// One point every five seconds: 120 points draw a smooth line and cost little to copy.
const TRAIL_STEP_MS: u64 = 5_000;

/// A miner this computer can name, with the address its rewards go to and its speed now.
struct Named {
    miner: Miner,
    address: String,
    hashrate: Option<u64>,
    /// On this node's chain, so its speed counts toward the next block.
    joined: bool,
}

fn named(node: &NodeView, local: &[LocalView], boards: &[BoardView]) -> Vec<Named> {
    let own_rate = node.mining.then_some(node.hashrate);
    let mut known: Vec<Named> = node
        .keys
        .iter()
        .map(|key| key.address.clone())
        .chain([node.miner.clone(), node.address.clone()])
        .filter(|address| !address.is_empty())
        .map(|address| Named { miner: Miner::You, address, hashrate: None, joined: true })
        .collect();
    if let Some(first) = known.iter_mut().find(|named| named.address == node.miner) {
        first.hashrate = own_rate;
    } else if let Some(first) = known.first_mut() {
        first.hashrate = own_rate;
    }
    for view in local {
        let Some(status) = &view.status else {
            continue;
        };
        known.push(Named {
            miner: Miner::Here(view.spec.name.clone()),
            address: status.address.clone(),
            hashrate: status.mining.then_some(status.hashrate),
            joined: view.spec.joined,
        });
    }
    for board in boards {
        let beacon = &board.seen.beacon;
        known.push(Named {
            miner: Miner::Board(beacon.name.clone()),
            address: format_address(&beacon.address),
            hashrate: (beacon.mining && board.seen.online).then_some(u64::from(beacon.hashrate)),
            joined: beacon.genesis == node.genesis,
        });
    }
    known
}

fn who(block: &MinedBlock, known: &[Named]) -> Miner {
    let Some(payee) = &block.payee else {
        return Miner::Unknown;
    };
    let found = known.iter().find(|named| named.address == *payee);
    found.map_or_else(|| Miner::Away(payee.clone()), |named| named.miner.clone())
}

/// The tips other miners on this computer and the boards hold that differ from this node's, for
/// the node to look for a branch that races its own.
pub fn other_tips(node: &NodeView, local: &[LocalView], boards: &[BoardView]) -> Vec<Hash> {
    let here = local.iter().filter_map(|view| view.status.as_ref()).filter_map(|status| hash_of(&status.tip));
    let heard = boards.iter().filter(|board| board.seen.beacon.genesis == node.genesis).map(|board| board.seen.beacon.tip);
    let mut tips: Vec<Hash> = here.chain(heard).filter(|tip| *tip != node.tip && *tip != [0; 32]).collect();
    tips.sort_unstable();
    tips.dedup();
    tips
}

fn hash_of(hex: &str) -> Option<Hash> {
    let bytes = hex.as_bytes();
    if bytes.len() != 64 {
        return None;
    }
    let mut hash = [0; 32];
    for (at, pair) in bytes.chunks(2).enumerate() {
        let text = std::str::from_utf8(pair).ok()?;
        hash[at] = u8::from_str_radix(text, 16).ok()?;
    }
    Some(hash)
}

/// This computer's speed every few seconds over the last ten minutes.
#[derive(Default)]
pub struct Trail {
    points: VecDeque<(u64, u64)>,
}

impl Trail {
    pub fn add(&mut self, now_ms: u64, rate: u64) {
        let due = self.points.back().is_none_or(|(at, _)| now_ms >= at + TRAIL_STEP_MS);
        if due {
            self.points.push_back((now_ms, rate));
        } else if let Some(last) = self.points.back_mut() {
            // The newest point follows the speed until the next one is due, so the line's end
            // is the number beside it.
            last.1 = rate;
        }
        while self.points.front().is_some_and(|(at, _)| at + TRAIL_MS < now_ms) {
            self.points.pop_front();
        }
    }

    pub fn points(&self) -> Vec<(u64, u64)> {
        self.points.iter().copied().collect()
    }
}

/// Blocks the measured speed and the real average read: one lucky block moves them by a
/// twentieth, and a change of speed shows within twenty blocks. The wait itself never assumes a
/// retarget rule: it divides the difficulty the chain asks for the next block (`next_difficulty`,
/// which the node's status carries) by the speed.
pub const RECENT: usize = 20;

/// The average time between the last [`RECENT`] blocks.
fn average_of(latest: &[MinedBlock]) -> Option<u64> {
    let recent = &latest[..latest.len().min(RECENT + 1)];
    let (newest, oldest) = (recent.first()?, recent.last()?);
    let span = newest.time_ms.checked_sub(oldest.time_ms).filter(|span| *span > 0)?;
    Some(span / (recent.len() as u64 - 1))
}

/// The speed the last [`RECENT`] blocks took: their work over the time between them.
fn speed_of_blocks(latest: &[MinedBlock]) -> Option<u64> {
    let latest = &latest[..latest.len().min(RECENT + 1)];
    let (newest, oldest) = (latest.first()?, latest.last()?);
    let span = newest.time_ms.checked_sub(oldest.time_ms).filter(|span| *span > 0)?;
    let work: u128 = latest[..latest.len() - 1].iter().map(|block| u128::from(block.difficulty)).sum();
    u64::try_from(work * 1_000 / u128::from(span)).ok()
}

/// Fills in who mined what, the shares, the speed line and the next block's wait.
pub fn complete(view: &mut ChainView, node: &NodeView, local: &[LocalView], boards: &[BoardView], trail: &mut Trail, now_ms: u64) {
    let known = named(node, local, boards);
    let branches = view.rival.iter_mut().chain(view.replaced.iter_mut()).flat_map(|branch| branch.blocks.iter_mut());
    for block in view.latest.iter_mut().chain(branches) {
        block.miner = who(block, &known);
    }
    let mut counts: BTreeMap<Miner, u32> = BTreeMap::new();
    for block in &view.latest {
        *counts.entry(block.miner.clone()).or_default() += 1;
    }
    let rate_of = |miner: &Miner| {
        let rates = known.iter().filter(|named| named.miner == *miner).filter_map(|named| named.hashrate);
        rates.reduce(|sum, rate| sum + rate)
    };
    let mut shares: Vec<Share> =
        counts.iter().map(|(miner, blocks)| Share { miner: miner.clone(), blocks: *blocks, hashrate: rate_of(miner) }).collect();
    for named in known.iter().filter(|named| named.joined && named.hashrate.is_some_and(|rate| rate > 0)) {
        if !shares.iter().any(|share| share.miner == named.miner) {
            shares.push(Share { miner: named.miner.clone(), blocks: 0, hashrate: rate_of(&named.miner) });
        }
    }
    shares.sort_by(|a, b| {
        let key = |share: &Share| (share.blocks, share.hashrate.unwrap_or(0), share.miner == Miner::You);
        key(b).cmp(&key(a))
    });
    view.shares = shares;

    let own = if node.mining { node.hashrate } else { 0 };
    let here: u64 = local.iter().filter_map(|view| view.status.as_ref()).filter(|status| status.mining).map(|status| status.hashrate).sum();
    view.hashrate_here = own + here;
    trail.add(now_ms, view.hashrate_here);
    view.trail = trail.points();

    let heard: u64 = known.iter().filter(|named| named.joined).filter_map(|named| named.hashrate).sum();
    let all_named = view.latest.iter().all(|block| !matches!(block.miner, Miner::Away(_) | Miner::Unknown));
    view.hashrate_all = match (all_named, speed_of_blocks(&view.latest)) {
        (false, Some(seen)) => heard.max(seen),
        _ => heard,
    };
    view.expected_ms = (view.hashrate_all > 0 && node.difficulty > 0)
        .then(|| u64::try_from(u128::from(node.difficulty) * 1_000 / u128::from(view.hashrate_all)).unwrap_or(u64::MAX));
    view.average_ms = average_of(&view.latest);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(height: u64, payee: &str, time_ms: u64) -> MinedBlock {
        MinedBlock { height, time_ms, difficulty: 1_000, payee: Some(payee.into()), ..MinedBlock::default() }
    }

    /// Two blocks of this wallet and one of an address nobody here knows: the wallet leads, the
    /// stranger is named by address, and the wait comes from the speed the blocks took.
    #[test]
    fn shares_name_each_miner_and_the_wait_follows_the_speed() {
        let node = NodeView {
            address: "olcmine".into(),
            miner: "olcmine".into(),
            mining: true,
            hashrate: 100,
            difficulty: 1_000,
            ..NodeView::default()
        };
        let mut view = ChainView {
            latest: vec![block(3, "olcmine", 3_000), block(2, "olcother", 2_000), block(1, "olcmine", 1_000)],
            ..ChainView::default()
        };
        let mut trail = Trail::default();
        complete(&mut view, &node, &[], &[], &mut trail, 50_000);
        assert_eq!(view.shares[0], Share { miner: Miner::You, blocks: 2, hashrate: Some(100) });
        assert_eq!(view.shares[1].miner, Miner::Away("olcother".into()));
        assert_eq!(view.window(), 3);
        // Two blocks of 1,000 in two seconds: 1,000 a second, more than this node's own 100.
        assert_eq!(view.hashrate_all, 1_000);
        assert_eq!(view.expected_ms, Some(1_000));
        assert_eq!(view.average_ms, Some(1_000));
        assert_eq!(view.trail, [(50_000, 100)]);
        complete(&mut view, &node, &[], &[], &mut trail, 50_000 + TRAIL_MS + 10_000);
        assert_eq!(view.trail.len(), 1, "points older than ten minutes go");
    }

    #[test]
    fn a_tip_reads_back_from_hex() {
        let tip = [7u8; 32];
        let hex: String = tip.iter().map(|byte| format!("{byte:02x}")).collect();
        assert_eq!(hash_of(&hex), Some(tip));
        assert_eq!(hash_of("zz"), None);
    }
}
