//! A coin changing its rules at a height, on real blocks: same genesis, same history, two chains.
mod common;
mod store;

use common::{engine_of, legacy_coin, mine, silence, test_coin};
use olc_core::types::{BITCOIN_VERSION, MARK_BITS};
use olc_core::Chain;
use olc_wallet::Wallet;
use store::{flash, keep_on, plugins};
use olc_core::chain::ChainError;
use olc_core::coin::{Fork, COIN, NEVER};
use olc_core::{Block, CoinConfig, Rules};
use olc_node::Engine;

const FORK_AT: u64 = 6;

fn forked(coin: &CoinConfig, change: impl Fn(&mut Rules)) -> CoinConfig {
    let mut rules = coin.rules_at(FORK_AT).clone();
    change(&mut rules);
    let mut new = coin.clone();
    new.forks.push(Fork { from_height: FORK_AT, rules });
    new
}

fn mine_to(node: &mut Engine, height: u64) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut now = node.chain().tip_header().time_ms + 1;
    while node.chain().height() < height {
        mine(node, now);
        silence(node);
        now += 1;
        let tip = node.chain().tip();
        blocks.extend(node.chain().block(&tip));
    }
    blocks
}

fn both(change: impl Fn(&mut Rules)) -> (Engine, Engine) {
    both_on(test_coin(), change)
}

fn both_on(coin: CoinConfig, change: impl Fn(&mut Rules)) -> (Engine, Engine) {
    let new = forked(&coin, change);
    (engine_of(1, coin), engine_of(2, new))
}

/// History is shared, then each side refuses the other's first block, whichever rule changed.
fn split_cleanly(change: impl Fn(&mut Rules)) {
    let (mut old, mut new) = both(change);
    for block in mine_to(&mut old, FORK_AT - 1) {
        new.chain_mut().add_block(block).unwrap();
    }
    assert_eq!(old.chain().tip(), new.chain().tip(), "the history is one history");

    let old_side = mine_to(&mut old, FORK_AT);
    let new_side = mine_to(&mut new, FORK_AT);
    assert_ne!(old.chain().tip(), new.chain().tip());
    let refused = |result| matches!(result, Err(ChainError::BadDifficulty));
    assert!(refused(new.chain_mut().add_block(old_side[0].clone())), "the fork ignores the old chain");
    assert!(refused(old.chain_mut().add_block(new_side[0].clone())), "the old chain ignores the fork");

    mine_to(&mut old, FORK_AT + 3);
    mine_to(&mut new, FORK_AT + 3);
    assert_eq!(old.chain().height(), FORK_AT + 3);
    assert_eq!(new.chain().height(), FORK_AT + 3);
}

#[test]
fn a_rule_only_one_side_would_notice_still_splits_both_ways() {
    split_cleanly(|rules| rules.reward *= 2);
    split_cleanly(|rules| rules.reward /= 2);
    split_cleanly(|rules| rules.min_fee = 5);
    split_cleanly(|rules| rules.max_block_bytes *= 2);
    split_cleanly(|rules| rules.block_time_ms = 200);
}

#[test]
fn a_fork_with_a_new_reward_pays_it_from_its_height_and_old_blocks_stay_valid() {
    let (_, mut new) = both(|rules| rules.reward = 7 * COIN);
    let blocks = mine_to(&mut new, FORK_AT + 1);
    let paid = |height: u64| blocks[height as usize - 1].txs[0].outputs[0].value;
    assert_eq!(paid(FORK_AT - 1), test_coin().rules.reward);
    assert_eq!(paid(FORK_AT), 7 * COIN);

    // Someone new takes the whole chain and judges each block by the rules of its height.
    let mut late = engine_of(3, new.chain().config().clone());
    for block in blocks {
        late.chain_mut().add_block(block).unwrap();
    }
    assert_eq!(late.chain().tip(), new.chain().tip());
}

/// On a coin from before Bitcoin's header the mark is added to the difficulty.
#[test]
fn the_mark_is_never_what_the_old_rules_ask() {
    let (mut old, mut new) = both_on(legacy_coin(), |rules| rules.halving_interval += 1);
    mine_to(&mut old, FORK_AT - 1);
    mine_to(&mut new, FORK_AT - 1);
    let tip = old.chain().tip();
    let at_tip = new.chain().tip_header().difficulty;
    let mark = new.chain().config().forks[0].mark();
    assert!((1..=256).contains(&mark));
    assert_eq!(old.chain().next_difficulty(&tip), at_tip, "the old rules ask the same again");
    let asked = new.chain().next_difficulty(&new.chain().tip());
    assert_eq!(asked, at_tip.max(new.chain().rules().floor_at(FORK_AT)) + mark);
}

/// On a coin with Bitcoin's header the mark sits in version bits no miner rolls, and the target
/// is the rule's.
#[test]
fn the_mark_goes_in_the_version_and_leaves_the_target() {
    let (mut old, mut new) = both(|rules| rules.halving_interval += 1);
    mine_to(&mut old, FORK_AT - 1);
    let blocks = mine_to(&mut new, FORK_AT);
    let mark = new.chain().config().forks[0].mark();
    let first = blocks[FORK_AT as usize - 1].header;
    assert_eq!(u64::from(first.version & MARK_BITS), mark);
    assert_eq!(first.version & !MARK_BITS, BITCOIN_VERSION);
    let floor = new.chain().rules().floor_at(FORK_AT);
    let asked = old.chain().next_difficulty(&old.chain().tip()).max(floor);
    assert_eq!(first.difficulty, olc_core::bits::rounded(asked), "the old rule's, raised to the floor");
    assert_eq!(new.chain().tip_header().difficulty, first.difficulty);
    assert_eq!(new.chain().header_version_at(FORK_AT + 1), BITCOIN_VERSION, "only the first block");
}

#[test]
fn a_raised_floor_starts_at_the_fork() {
    let (_, mut new) = both_on(legacy_coin(), |rules| {
        rules.min_difficulty = 40;
        rules.floor_from_height = 0;
    });
    let blocks = mine_to(&mut new, FORK_AT);
    let mark = new.chain().config().forks[0].mark();
    assert_eq!(blocks[FORK_AT as usize - 1].header.difficulty, 40 + mark);

    let (_, mut new) = both(|rules| rules.min_difficulty = 40_000);
    let blocks = mine_to(&mut new, FORK_AT);
    assert_eq!(blocks[FORK_AT as usize - 1].header.difficulty, olc_core::bits::rounded(40_000));
}

#[test]
fn a_coin_without_forks_keeps_its_exact_bytes() {
    let ended = |coin: &CoinConfig| {
        let mut bytes = postcard::to_allocvec(coin).unwrap();
        let end = (coin.rules.min_difficulty, coin.message_in_block, coin.rules.floor_from_height);
        bytes.extend(postcard::to_allocvec(&end).unwrap());
        bytes
    };
    let mut old = legacy_coin();
    old.upgrade_from = NEVER;
    assert_eq!(old.to_bytes(), ended(&old), "not a byte more than before forks");
    assert_eq!(CoinConfig::from_bytes(&ended(&old)), Some(old));
    // OLC's difficulty rule, time rules and header come after what older builds read.
    let olc = CoinConfig::olc();
    let bytes = olc.to_bytes();
    assert!(bytes.len() > ended(&olc).len());
    assert_eq!(CoinConfig::from_bytes(&bytes), Some(olc));
}

#[test]
fn a_fork_travels_past_where_older_builds_stop_reading() {
    let coin = forked(&legacy_coin(), |rules| {
        rules.reward = 3;
        rules.min_difficulty = 77;
        rules.floor_from_height = 9;
    });
    let bytes = coin.to_bytes();
    assert_eq!(CoinConfig::from_bytes(&bytes).as_ref(), Some(&coin));
    let (seen_by_old, _) = postcard::take_from_bytes::<CoinConfig>(&bytes).unwrap();
    assert_eq!(seen_by_old.genesis().id(), coin.genesis().id());
    assert_eq!(seen_by_old.rules.reward, coin.rules.reward, "the old side sees the rules it has");

    // An older build reads a coin with Bitcoin's header as another coin, so it never follows it.
    let new = forked(&test_coin(), |rules| rules.reward = 3);
    let (seen_by_old, _) = postcard::take_from_bytes::<CoinConfig>(&new.to_bytes()).unwrap();
    assert_ne!(seen_by_old.genesis().id(), new.genesis().id());
    assert_eq!(CoinConfig::from_bytes(&new.to_bytes()), Some(new));
}

/// A board moved between sides finds blocks of the side it left. They go, or it would stand on
/// neither side: its saved state is trusted, and the rules are only asked of new blocks.
#[test]
fn a_move_between_sides_leaves_the_blocks_of_the_side_it_left() {
    let coin = test_coin();
    let new = forked(&coin, |rules| rules.min_fee += 1);
    assert_eq!(coin.parts_from(&new), Some(FORK_AT));
    assert_eq!(new.parts_from(&coin), Some(FORK_AT));
    assert_eq!(coin.parts_from(&coin), None);

    let drive = flash(1 << 20);
    let wallet = || Wallet::from_secret([1; 32]).unwrap();
    let open = |coin: &CoinConfig| Chain::open(coin.clone(), plugins(), keep_on(&drive, None));
    let mut old_side = Engine::on_chain(open(&coin), wallet());
    mine_to(&mut old_side, FORK_AT + 3);
    let kept = old_side.chain().id_at(FORK_AT - 1).unwrap();
    assert_eq!(old_side.chain_mut().step_down_below(FORK_AT).unwrap(), 4);
    drop(old_side);

    let mut fork = Engine::on_chain(open(&new), wallet());
    assert_eq!(fork.chain().tip(), kept, "the boot walked back to the blocks that went");
    mine_to(&mut fork, FORK_AT + 2);
    let marked = fork.chain().id_at(FORK_AT).unwrap();
    assert_eq!(fork.chain_mut().step_down_below(FORK_AT).unwrap(), 3);
    drop(fork);

    let mut back = Engine::on_chain(open(&coin), wallet());
    assert_eq!(back.chain().tip(), kept);
    mine_to(&mut back, FORK_AT);
    assert_ne!(back.chain().tip(), marked, "this side knows nothing of the mark");
    assert_eq!(back.status().height, FORK_AT);
    drop(back);

    let reopened = open(&coin);
    assert_eq!(reopened.height(), FORK_AT, "the log still reads after a step down");
}

#[test]
fn a_rules_change_above_the_tip_takes_nothing() {
    let coin = test_coin();
    let mut node = engine_of(1, coin);
    mine_to(&mut node, FORK_AT - 2);
    assert_eq!(node.chain_mut().step_down_below(FORK_AT).unwrap(), 0);
    assert_eq!(node.status().height, FORK_AT - 2);
}

#[test]
fn a_chain_reopened_under_a_fork_keeps_its_history() {
    let coin = test_coin();
    let mut node = engine_of(1, coin.clone());
    let blocks = mine_to(&mut node, FORK_AT - 1);
    let new = forked(&coin, |rules| rules.reward = 1);
    let mut fork = engine_of(1, new);
    for block in blocks {
        fork.chain_mut().add_block(block).unwrap();
    }
    mine_to(&mut fork, FORK_AT);
    assert_eq!(fork.chain().config().before_last_fork().as_ref(), Some(&coin));
    assert!(fork.chain().config().forks_id().len() == 8);
    assert!(coin.forks_id().is_empty());
}
