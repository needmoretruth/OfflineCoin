//! Names the miner of every block from what this computer knows: its own wallet, the miners it
//! runs, the boards it hears. Then counts each one's share, keeps this computer's speed over ten
//! minutes and works out when the next block should come.
use super::chain_view::{ChainView, MinedBlock, Miner, Reading, Share, SHARE_WINDOW};
use super::pool_plan::Who;
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
    // Work for a pool hashes with mining off, so a measured speed counts whatever the switch says.
    let own_rate = (node.mining || node.hashrate > 0).then_some(node.hashrate);
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
            hashrate: (status.mining || status.hashrate > 0).then_some(status.hashrate),
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

/// A block's finder. A pool's block is the pool's, as Bitcoin's explorers count them: this app's
/// pool when it pays this app's wallet, another pool by the address it pays otherwise.
fn who(block: &MinedBlock, known: &[Named]) -> Miner {
    let Some(payee) = &block.payee else {
        return Miner::Unknown;
    };
    let found = known.iter().find(|named| named.address == *payee).map(|named| named.miner.clone());
    match (block.pool, found) {
        (true, Some(Miner::You)) => Miner::OwnPool,
        (true, _) => Miner::Pool(payee.clone()),
        (false, found) => found.unwrap_or_else(|| Miner::Away(payee.clone())),
    }
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

/// Blocks the real average time between blocks reads. The wait itself never assumes a retarget
/// rule: it divides the difficulty the chain asks for the next block (`next_difficulty`, which
/// the node's status carries) by the network's speed.
pub const RECENT: usize = 20;

/// Fewer blocks than this give no network speed. Block times scatter, so a speed read from n
/// blocks is off by about one part in √n: twelve keep that under 30 %. Bitcoin's getnetworkhashps
/// reads 120 by default, a day of its blocks; a coin here may be minutes old, so it would show
/// nothing for most of its life. The block before the twelve marks where their time starts.
pub const FEWEST_BLOCKS: usize = 12;

/// The network's speed reads every block of this last stretch, up to now, when it holds more than
/// [`FEWEST_BLOCKS`]; so a change of speed shows in full half a minute after it, however many
/// blocks are kept. When blocks come slower, the last [`FEWEST_BLOCKS`] stretch it further.
pub const SPAN_MS: u64 = 30_000;

/// The average time between the last [`RECENT`] blocks.
fn average_of(latest: &[MinedBlock]) -> Option<u64> {
    let recent = &latest[..latest.len().min(RECENT + 1)];
    let (newest, oldest) = (recent.first()?, recent.last()?);
    let span = newest.time_ms.checked_sub(oldest.time_ms).filter(|span| *span > 0)?;
    Some(span / (recent.len() as u64 - 1))
}

/// The speed the last `count` blocks took: their work over the time between them, as Bitcoin
/// Core's getnetworkhashps reads it. A difficulty of N asks about N tries.
fn speed_of_blocks(latest: &[MinedBlock], count: usize) -> Option<u64> {
    let latest = &latest[..latest.len().min(count + 1)];
    let (newest, oldest) = (latest.first()?, latest.last()?);
    let span = newest.time_ms.checked_sub(oldest.time_ms).filter(|span| *span > 0)?;
    let work: u128 = latest[..latest.len() - 1].iter().map(|block| u128::from(block.difficulty)).sum();
    u64::try_from(work * 1_000 / u128::from(span)).ok()
}

/// The network's speed now: the work of the blocks of the last [`SPAN_MS`], or of the last
/// [`FEWEST_BLOCKS`] when that holds fewer, over their time up to now. Counting up to now, not
/// up to the newest block, lets a slowdown show before the next block comes.
fn speed_now(latest: &[MinedBlock], now_ms: u64) -> Option<Reading> {
    let end = now_ms.max(latest.first()?.time_ms);
    let in_span = latest.iter().take_while(|block| block.time_ms.saturating_add(SPAN_MS) > end).count();
    let blocks = in_span.max(FEWEST_BLOCKS).min(latest.len().saturating_sub(1));
    if blocks < FEWEST_BLOCKS {
        return None;
    }
    // The span itself when it holds the blocks and an older one lies outside it; otherwise the
    // block before the counted ones starts their time.
    let start = if blocks == in_span { end.saturating_sub(SPAN_MS) } else { latest[blocks].time_ms };
    let span_ms = end.checked_sub(start).filter(|span| *span > 0)?;
    let work: u128 = latest[..blocks].iter().map(|block| u128::from(block.difficulty)).sum();
    let rate = u64::try_from(work * 1_000 / u128::from(span_ms)).ok()?;
    Some(Reading { blocks, span_ms, rate })
}

/// A miner here working in a pool: the one this app runs, or another whose blocks pay `payee`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InPool {
    pub worker: Who,
    pub here: bool,
    /// The address the pool's blocks pay, as the worker read it from its work.
    pub payee: Option<String>,
}

fn miner_of(who: &Who, local: &[LocalView], boards: &[BoardView]) -> Option<Miner> {
    let board = |found: Option<&BoardView>| found.map(|board| Miner::Board(board.seen.beacon.name.clone()));
    match who {
        Who::Own => Some(Miner::You),
        Who::Node(id) => local.iter().find(|view| view.spec.id == *id).map(|view| Miner::Here(view.spec.name.clone())),
        Who::Board(id) => board(boards.iter().find(|board| board.seen.id == *id)),
        Who::Lent(address) => board(boards.iter().find(|board| board.seen.beacon.address == *address)),
    }
}

/// Each miner here that works in a pool beside the pool it works in, named as its blocks are.
fn working(pools: &[InPool], known: &[Named], local: &[LocalView], boards: &[BoardView]) -> Vec<(Miner, Miner)> {
    let mut pairs = Vec::new();
    for in_pool in pools {
        let pays_you = |payee: &String| known.iter().any(|named| named.miner == Miner::You && named.address == *payee);
        let pool = match &in_pool.payee {
            _ if in_pool.here => Miner::OwnPool,
            Some(payee) if pays_you(payee) => Miner::OwnPool,
            Some(payee) => Miner::Pool(payee.clone()),
            None => continue,
        };
        if let Some(worker) = miner_of(&in_pool.worker, local, boards) {
            pairs.push((worker, pool));
        }
    }
    pairs
}

/// Fills in who mined what, the shares, the speed line and the next block's wait. A miner that
/// works in a pool finds blocks for the pool, so it is listed under the pool's row rather than as
/// a finder of its own; one that found blocks itself before it joined keeps its row for them.
pub fn complete(view: &mut ChainView, node: &NodeView, local: &[LocalView], boards: &[BoardView], pools: &[InPool], trail: &mut Trail, now_ms: u64) {
    let known = named(node, local, boards);
    let working = working(pools, &known, local, boards);
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
    let share_of = |miner: &Miner, blocks: u32| Share { miner: miner.clone(), blocks, hashrate: rate_of(miner), workers: Vec::new() };
    let mut shares: Vec<Share> = counts.iter().map(|(miner, blocks)| share_of(miner, *blocks)).collect();
    let in_a_pool = |miner: &Miner| working.iter().any(|(worker, _)| worker == miner);
    for named in known.iter().filter(|named| named.joined && named.hashrate.is_some_and(|rate| rate > 0)) {
        if !in_a_pool(&named.miner) && !shares.iter().any(|share| share.miner == named.miner) {
            shares.push(share_of(&named.miner, 0));
        }
    }
    for (worker, pool) in &working {
        if !shares.iter().any(|share| share.miner == *pool) {
            shares.push(share_of(pool, 0));
        }
        if let Some(share) = shares.iter_mut().find(|share| share.miner == *pool) {
            if !share.workers.contains(worker) {
                share.workers.push(worker.clone());
            }
        }
    }
    shares.sort_by(|a, b| {
        let key = |share: &Share| (share.blocks, share.hashrate.unwrap_or(0), share.miner == Miner::You);
        key(b).cmp(&key(a))
    });
    view.shares = shares;

    let here: u64 = local.iter().filter_map(|view| view.status.as_ref()).map(|status| status.hashrate).sum();
    view.hashrate_here = node.hashrate + here;
    trail.add(now_ms, view.hashrate_here);
    view.trail = trail.points();

    // The network's speed is what the blocks took, whoever made them: a pool's workers and a
    // stranger count as much as a miner this computer hears. It holds this computer, so it never
    // reads below what this computer measures. Until enough blocks are kept, the speeds heard
    // stand in for the wait, and the screens say the network's speed is not known yet.
    let heard: u64 = known.iter().filter(|named| named.joined).filter_map(|named| named.hashrate).sum();
    view.network = speed_now(&view.latest, now_ms);
    view.hashrate_all = view.network.map_or(heard, |reading| reading.rate).max(view.hashrate_here);
    let enough = view.latest.len() > FEWEST_BLOCKS;
    view.hashrate_average = speed_of_blocks(&view.latest, SHARE_WINDOW).filter(|_| enough).unwrap_or(0);
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
    /// stranger is named by address. Three blocks are too few to read the network's speed from,
    /// so the wait comes from the speed heard.
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
        complete(&mut view, &node, &[], &[], &[], &mut trail, 50_000);
        assert_eq!(view.shares[0], Share { miner: Miner::You, blocks: 2, hashrate: Some(100), workers: Vec::new() });
        assert_eq!(view.shares[1].miner, Miner::Away("olcother".into()));
        assert_eq!(view.window(), 3);
        assert_eq!(view.network, None);
        assert_eq!((view.hashrate_all, view.hashrate_average), (100, 0));
        assert_eq!(view.expected_ms, Some(10_000));
        assert_eq!(view.average_ms, Some(1_000));
        assert_eq!(view.trail, [(50_000, 100)]);
        complete(&mut view, &node, &[], &[], &[], &mut trail, 50_000 + TRAIL_MS + 10_000);
        assert_eq!(view.trail.len(), 1, "points older than ten minutes go");
    }

    /// Blocks keep coming to this wallet while nothing here says it hashes, as when boards work
    /// in this computer's pool: the network's speed is what the blocks took, as Bitcoin's
    /// getnetworkhashps reads it, never 0 while blocks come.
    #[test]
    fn the_network_speed_comes_from_the_blocks_when_every_miner_is_known() {
        let node = NodeView { address: "olcmine".into(), miner: "olcmine".into(), difficulty: 1_000, ..NodeView::default() };
        let latest: Vec<MinedBlock> = (1..=21).rev().map(|height| block(height, "olcmine", height * 2_000)).collect();
        let mut view = ChainView { latest, ..ChainView::default() };
        complete(&mut view, &node, &[], &[], &[], &mut Trail::default(), 43_000);
        // Fifteen blocks of 1,000 tries in the last thirty seconds.
        assert_eq!(view.hashrate_all, 500, "the network's speed while blocks come");
        assert_eq!(view.network, Some(Reading { blocks: 15, span_ms: SPAN_MS, rate: 500 }));
        assert_eq!(view.expected_ms, Some(2_000));
        // Twenty blocks of 1,000 in forty seconds, as getnetworkhashps reads every block kept.
        assert_eq!(view.hashrate_average, 500);
    }

    /// Blocks every two seconds, then every second once the network doubles its speed: half a
    /// minute later the speed now reads only the fast blocks, while forty blocks would still hold
    /// slow ones. With no new block for long, the speed falls before the next one comes.
    #[test]
    fn the_network_speed_follows_a_change_within_its_span() {
        let node = NodeView { address: "olcmine".into(), miner: "olcmine".into(), difficulty: 1_000, ..NodeView::default() };
        let slow = (1..=20).map(|height| block(height, "olcmine", height * 2_000));
        let fast = (21..=50).map(|height| block(height, "olcmine", 40_000 + (height - 20) * 1_000));
        let latest: Vec<MinedBlock> = slow.chain(fast).rev().collect();
        let mut view = ChainView { latest, ..ChainView::default() };
        complete(&mut view, &node, &[], &[], &[], &mut Trail::default(), 70_000);
        assert_eq!(view.hashrate_all, 1_000, "thirty fast blocks in thirty seconds");
        assert_eq!(speed_of_blocks(&view.latest, 40), Some(800), "forty blocks still hold slow ones");
        complete(&mut view, &node, &[], &[], &[], &mut Trail::default(), 130_000);
        // Nothing for a minute: the last twelve blocks over the seventy-two seconds since the
        // block before them.
        assert_eq!(view.network, Some(Reading { blocks: FEWEST_BLOCKS, span_ms: 72_000, rate: 166 }));
        assert_eq!(view.hashrate_all, 12_000 * 1_000 / 72_000);
    }

    /// The network holds this computer, so its speed never reads below what this computer
    /// measures, however slow the last blocks were; and twelve blocks are the fewest it reads.
    #[test]
    fn the_network_speed_is_never_below_this_computer() {
        let node = NodeView { address: "olcmine".into(), miner: "olcmine".into(), mining: true, hashrate: 900, difficulty: 1_000, ..NodeView::default() };
        let latest: Vec<MinedBlock> = (1..=13).rev().map(|height| block(height, "olcmine", height * 2_000)).collect();
        let mut view = ChainView { latest, ..ChainView::default() };
        complete(&mut view, &node, &[], &[], &[], &mut Trail::default(), 26_000);
        assert_eq!(view.network, Some(Reading { blocks: 12, span_ms: 24_000, rate: 500 }), "twelve blocks after the first");
        assert_eq!(view.hashrate_all, 900, "the blocks took 500 a second, this computer measures 900");
        view.latest.pop();
        complete(&mut view, &node, &[], &[], &[], &mut Trail::default(), 26_000);
        assert_eq!(view.network, None, "eleven blocks after the first are too few");
    }

    /// A pool's tagged blocks are the pool's: this app's pool when they pay this app's wallet,
    /// another pool by its address otherwise. Four blocks are too few for the network's speed.
    #[test]
    fn a_pools_blocks_are_named_for_the_pool() {
        let node = NodeView { address: "olcmine".into(), miner: "olcmine".into(), difficulty: 1_000, ..NodeView::default() };
        let pooled = |height, payee: &str| MinedBlock { pool: true, ..block(height, payee, height * 1_000) };
        let latest = vec![pooled(4, "olcmine"), pooled(3, "olcpool"), block(2, "olcmine", 2_000), block(1, "olcmine", 1_000)];
        let mut view = ChainView { latest, ..ChainView::default() };
        complete(&mut view, &node, &[], &[], &[], &mut Trail::default(), 50_000);
        let miners: Vec<&Miner> = view.latest.iter().map(|block| &block.miner).collect();
        assert_eq!(miners, [&Miner::OwnPool, &Miner::Pool("olcpool".into()), &Miner::You, &Miner::You]);
        assert_eq!((view.network, view.hashrate_average), (None, 0));
    }

    /// A miner here working in this app's pool, and this app working in another computer's pool,
    /// are listed under those pools' rows, not as finders of their own with no blocks.
    #[test]
    fn a_miner_in_a_pool_is_listed_under_the_pool() {
        let node = NodeView {
            address: "olcmine".into(),
            miner: "olcmine".into(),
            hashrate: 300,
            difficulty: 1_000,
            ..NodeView::default()
        };
        let spec = crate::local::NodeSpec { id: 7, name: "miner-b".into(), joined: true, ..crate::local::NodeSpec::default() };
        let status = crate::local::ChildStatus { address: "olcb".into(), hashrate: 200, ..crate::local::ChildStatus::default() };
        let local = [LocalView {
            spec,
            pid: Some(1),
            status: Some(status),
            measured: None,
            memory_limit: 0,
            enforcement: crate::local::Enforcement::Nothing,
            ended: None,
        }];
        let pooled = |height, payee: &str| MinedBlock { pool: true, ..block(height, payee, height * 1_000) };
        let latest = vec![pooled(3, "olcmine"), pooled(2, "olcfar"), block(1, "olcmine", 1_000)];
        let mut view = ChainView { latest, ..ChainView::default() };
        let pools = [
            InPool { worker: Who::Node(7), here: true, payee: None },
            InPool { worker: Who::Own, here: false, payee: Some("olcfar".into()) },
        ];
        complete(&mut view, &node, &local, &[], &pools, &mut Trail::default(), 50_000);
        let rows: Vec<(&Miner, u32, &[Miner])> = view.shares.iter().map(|share| (&share.miner, share.blocks, share.workers.as_slice())).collect();
        assert!(rows.contains(&(&Miner::OwnPool, 1, &[Miner::Here("miner-b".into())][..])), "{rows:?}");
        assert!(rows.contains(&(&Miner::Pool("olcfar".into()), 1, &[Miner::You][..])), "{rows:?}");
        assert!(!rows.iter().any(|(miner, _, _)| **miner == Miner::Here("miner-b".into())), "no 0 % row of its own: {rows:?}");
        // This app found a block alone before it joined, and keeps its row for it.
        assert!(rows.contains(&(&Miner::You, 1, &[][..])), "{rows:?}");
    }

    #[test]
    fn a_tip_reads_back_from_hex() {
        let tip = [7u8; 32];
        let hex: String = tip.iter().map(|byte| format!("{byte:02x}")).collect();
        assert_eq!(hash_of(&hex), Some(tip));
        assert_eq!(hash_of("zz"), None);
    }
}
