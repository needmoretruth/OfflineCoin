//! The three difficulty rules and the time rules on a real chain: blocks are built, sealed and
//! taken as a node takes them, and a header that breaks a rule is refused.
use olc_core::bits::rounded;
use olc_core::coin::Fork;
use olc_core::types::{BITCOIN_VERSION, MARK_BITS};
use olc_core::{Block, BlockHeader, Chain, ChainError, CoinConfig, Consensus, DifficultyRule, Plugins};
use olc_pow::ProofOfWork;

const T: u64 = 30_000;
const START: u64 = 10_000;

fn coin(rule: DifficultyRule) -> CoinConfig {
    let mut coin = CoinConfig::olc();
    coin.name = "Rules".into();
    coin.rules.difficulty_rule = rule;
    coin.rules.retarget_interval = 10;
    coin.rules.min_difficulty = 1;
    coin.initial_difficulty = START;
    coin
}

fn chain(coin: CoinConfig) -> Chain {
    Chain::new(coin, Plugins { consensus: Box::new(ProofOfWork), languages: Vec::new() })
}

/// A block on the tip at `time_ms` as a miner builds it, sealed.
fn build(chain: &Chain, time_ms: u64) -> Block {
    let mut block = chain.block_template(&[9; 20], &[], time_ms);
    assert_eq!(block.header.time_ms, time_ms, "the template keeps the time asked for");
    assert!(ProofOfWork.try_seal(&mut block.header, 100_000_000));
    block
}

fn mine(chain: &mut Chain, time_ms: u64) -> BlockHeader {
    let block = build(chain, time_ms);
    let header = block.header;
    chain.add_block(block).unwrap();
    header
}

/// `block` with its header changed by `change` and sealed again.
fn changed(mut block: Block, change: impl Fn(&mut BlockHeader)) -> Block {
    change(&mut block.header);
    block.header.nonce = 0;
    assert!(ProofOfWork.try_seal(&mut block.header, 100_000_000));
    block
}

#[test]
fn asert_halves_after_a_block_one_half_life_late_and_rises_after_a_fast_one() {
    let coin = coin(DifficultyRule::Asert);
    let half_life = coin.rules.half_life_blocks * T;
    let mut chain = chain(coin.clone());
    let first = mine(&mut chain, coin.genesis_time_ms + T);
    assert_eq!(first.difficulty, rounded(START));
    let on_time = mine(&mut chain, first.time_ms + T);
    assert_eq!(on_time.difficulty, first.difficulty, "a block on time keeps the difficulty");
    let late = mine(&mut chain, on_time.time_ms + T + half_life);
    assert_eq!(late.difficulty, on_time.difficulty, "the late block's own difficulty was set before it");
    let after_late = mine(&mut chain, late.time_ms + T);
    assert_eq!(after_late.difficulty, rounded(late.difficulty / 2));
    let fast = mine(&mut chain, after_late.time_ms + 1_000);
    let after_fast = mine(&mut chain, fast.time_ms + T);
    // 29 seconds early of 180: 2^(29/180), about 1.118 times.
    let ratio = after_fast.difficulty as f64 / fast.difficulty as f64;
    assert!((ratio - 2f64.powf(29.0 / 180.0)).abs() < 0.001, "{ratio}");

    let wrong = changed(build(&chain, after_fast.time_ms + T), |header| header.difficulty = rounded(after_fast.difficulty * 2));
    assert_eq!(chain.add_block(wrong), Err(ChainError::BadDifficulty));
}

#[test]
fn bip94_gives_the_floor_after_two_block_times_and_retargets_from_the_window_start() {
    let coin = coin(DifficultyRule::Bip94);
    let mut chain = chain(coin.clone());
    let floor = rounded(1);
    let first = mine(&mut chain, coin.genesis_time_ms + T);
    assert_eq!(first.difficulty, rounded(START));
    // Two block times exactly is not yet more than two.
    assert_eq!(chain.next_difficulty_at(&chain.tip(), first.time_ms + 2 * T), first.difficulty);
    let slow = mine(&mut chain, first.time_ms + 2 * T + 1_000);
    assert_eq!(slow.difficulty, floor);
    // The next block on time carries the last difficulty that was not the exception's.
    let mut last = mine(&mut chain, slow.time_ms + T);
    assert_eq!(last.difficulty, first.difficulty);
    let too_easy = changed(build(&chain, last.time_ms + T), |header| header.difficulty = floor);
    assert_eq!(chain.add_block(too_easy), Err(ChainError::BadDifficulty));
    while chain.height() < 8 {
        last = mine(&mut chain, last.time_ms + T);
    }
    // Block 9 takes the exception, as a testnet3 miner would before a retarget.
    let ninth = mine(&mut chain, last.time_ms + 20 * T);
    assert_eq!(ninth.difficulty, floor);

    // Block 10 starts a window: no exception, and a retarget from block 0's difficulty, not
    // block 9's floor. The window ran slow, so it eases, by at most four times.
    let late = chain.next_difficulty_at(&chain.tip(), ninth.time_ms + 3 * T);
    assert!(late >= rounded(START / 4) && late < rounded(START), "{late}");
    // BIP 94's time warp rule: at most one block time before its parent.
    assert_eq!(chain.earliest_time(&chain.tip()), ninth.time_ms - T);
    let warped = changed(build(&chain, ninth.time_ms), |header| {
        header.time_ms = ninth.time_ms - T - 1_000;
        header.difficulty = late;
    });
    assert_eq!(chain.add_block(warped), Err(ChainError::BadTime));
    let tenth = mine(&mut chain, ninth.time_ms - T);
    assert!(tenth.difficulty >= rounded(START / 4));
}

#[test]
fn bitcoins_rule_has_no_exception_and_retargets_from_the_last_block() {
    let coin = coin(DifficultyRule::Bitcoin);
    let mut chain = chain(coin.clone());
    let mut last = mine(&mut chain, coin.genesis_time_ms + T);
    let slow = mine(&mut chain, last.time_ms + 40 * T);
    assert_eq!(slow.difficulty, last.difficulty, "between retargets the difficulty stands");
    last = slow;
    while chain.height() < 9 {
        last = mine(&mut chain, last.time_ms + T);
    }
    assert_eq!(last.difficulty, rounded(START));
    // Nine gaps took 48 block times against nine: it eases by the most it may, four times.
    let tenth = mine(&mut chain, last.time_ms + T);
    assert_eq!(tenth.difficulty, rounded(rounded(START) / 4));
    assert_eq!(chain.earliest_time(&chain.tip()) % 1_000, 0, "whole seconds");
}

#[test]
fn a_block_must_pass_the_middle_time_of_the_eleven_before_it() {
    let coin = coin(DifficultyRule::Asert);
    let mut chain = chain(coin.clone());
    let mut times = vec![coin.genesis_time_ms];
    while chain.height() < 11 {
        times.push(mine(&mut chain, times.last().unwrap() + T).time_ms);
    }
    // The eleven before height 12 are heights 1 to 11, whose middle is height 6.
    let middle = times[6];
    assert_eq!(chain.earliest_time(&chain.tip()), middle + 1_000);
    let at_middle = changed(build(&chain, middle + 1_000), |header| header.time_ms = middle);
    assert_eq!(chain.add_block(at_middle), Err(ChainError::BadTime));
    // Before its parent but after the middle is fine, as in Bitcoin.
    let earlier = mine(&mut chain, middle + 1_000);
    assert!(earlier.time_ms < times[11]);
    let not_whole = changed(build(&chain, earlier.time_ms + T), |header| header.time_ms += 500);
    assert_eq!(chain.add_block(not_whole), Err(ChainError::BadSeal), "Bitcoin's header holds whole seconds");
}

#[test]
fn a_fork_marks_its_first_block_in_the_version_and_leaves_the_target_alone() {
    let mut coin = coin(DifficultyRule::Asert);
    let mut changed_rules = coin.rules.clone();
    changed_rules.reward /= 2;
    coin.forks.push(Fork { from_height: 3, rules: changed_rules });
    let mut chain = chain(coin.clone());
    let first = mine(&mut chain, coin.genesis_time_ms + T);
    let second = mine(&mut chain, first.time_ms + T);
    assert_eq!(second.version, BITCOIN_VERSION);
    let block = build(&chain, second.time_ms + T);
    let marked = block.header.version;
    assert_ne!(marked, BITCOIN_VERSION);
    assert_eq!(marked & !MARK_BITS, BITCOIN_VERSION);
    assert_eq!(block.header.difficulty, second.difficulty, "the target is the rule's");
    let unmarked = changed(block.clone(), |header| header.version = BITCOIN_VERSION);
    assert_eq!(chain.add_block(unmarked), Err(ChainError::BadDifficulty));
    // Bits a miner rolls under BIP 320 do not change the mark.
    let rolled = changed(block, |header| header.version |= 0x0012_4000);
    chain.add_block(rolled).unwrap();
    assert_eq!(chain.height(), 3);
}

#[test]
fn a_coin_from_before_keeps_its_header_and_its_rules() {
    let mut coin = coin(DifficultyRule::Bitcoin);
    coin.bitcoin_header = false;
    coin.set_timings(&[olc_core::timing::LEGACY]);
    let mut chain = chain(coin.clone());
    let first = mine(&mut chain, coin.genesis_time_ms + 1);
    assert_eq!(first.version, 1);
    assert_eq!(first.difficulty, START, "a difficulty as it was, not rounded");
    let block = build(&chain, first.time_ms + 1);
    assert_eq!(block.header.sealing().bytes(), block.header.encode().as_slice(), "the 88 bytes are hashed");
    let same_time = changed(block.clone(), |header| header.time_ms = first.time_ms);
    assert_eq!(chain.add_block(same_time), Err(ChainError::BadTime));
    chain.add_block(block).unwrap();
}
