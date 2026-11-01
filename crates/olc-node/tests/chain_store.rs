//! The block store under real blocks: reorganisations against a full replay, a power cut at every
//! write, and everything a pruned node has to stay able to do.
mod store;

use olc_core::{AddOutcome, Chain, Opened, Served};
use olc_wallet::Wallet;
use store::*;

/// Added to a second per block, so the difficulty stays where it is.
const ON_TIME: u64 = 29_000;

fn feed(chain: &mut Chain, blocks: &[olc_core::Block]) {
    for block in blocks {
        let _ = chain.add_block(block.clone());
    }
}

#[test]
fn undo_lands_on_the_same_coins_as_a_full_replay() {
    for seed in [3, 77, 1234, 99991, 5, 2024] {
        let blocks = forking_blocks(30, seed);
        let mut chain = Chain::new(coin(), plugins());
        let mut moved = 0;
        for block in &blocks {
            if let Ok(AddOutcome::Reorganized { replaced }) = chain.add_block(block.clone()) {
                assert!(replaced > 0);
                moved += 1;
                let full = replayed(&chain);
                assert_eq!(full.tip(), chain.tip());
                assert_eq!(full.unspent_hash(), chain.unspent_hash());
            }
        }
        assert!(moved > 0, "seed {seed} never reorganised");
        assert_eq!(replayed(&chain).unspent_hash(), chain.unspent_hash());
    }
}

#[test]
fn a_cut_at_any_write_loses_at_most_the_last_block() {
    let blocks = forking_blocks(24, 41);
    let size = 1024 * 1024;

    // A run with no cut says how many writes there are and where the chain ends.
    let whole = flash(size);
    let mut chain = Chain::open(coin(), plugins(), keep_on(&whole, None));
    for (number, block) in blocks.iter().enumerate() {
        let _ = chain.add_block(block.clone());
        chain.keep_up(1);
        if number % 25 == 24 {
            chain.save_now();
        }
    }
    chain.save_now();
    let (end_tip, end_hash) = (chain.tip(), chain.unspent_hash());
    let steps = whole.lock().unwrap().steps;
    assert!(steps > blocks.len() as u64);

    for cut in 1..=steps {
        let drive = flash(size);
        drive.lock().unwrap().cut_at = Some(cut);
        let mut chain = Chain::open(coin(), plugins(), keep_on(&drive, None));
        let mut last_good = 0;
        let mut taken = 0;
        for (number, block) in blocks.iter().enumerate() {
            if drive.lock().unwrap().dead {
                break;
            }
            let height = chain.height();
            let _ = chain.add_block(block.clone());
            chain.keep_up(1);
            if number % 25 == 24 {
                chain.save_now();
            }
            if !drive.lock().unwrap().dead {
                last_good = height.max(chain.height());
                taken += 1;
            }
        }
        chain.save_now();
        let back = powered_again(&drive);
        let mut chain = Chain::open(coin(), plugins(), keep_on(&back, None));
        assert!(
            chain.height() + 1 >= last_good,
            "cut {cut}: height {} after {last_good}",
            chain.height()
        );
        let full = replayed(&chain);
        assert_eq!(full.tip(), chain.tip(), "cut {cut}");
        assert_eq!(full.unspent_hash(), chain.unspent_hash(), "cut {cut}");

        // Back on its feet: every block still comes in, and the chain ends where it would have.
        feed(&mut chain, &blocks[taken.min(blocks.len())..]);
        feed(&mut chain, &blocks);
        chain.save_now();
        assert_eq!(chain.tip(), end_tip, "cut {cut}");
        assert_eq!(chain.unspent_hash(), end_hash, "cut {cut}");
    }
}

fn long_chain(flash: &Flash, blocks: u64, recent: Option<u64>) -> Chain {
    let wallet = Wallet::from_secret([9; 32]).unwrap();
    let mut chain = Chain::open(coin(), plugins(), keep_on(flash, recent));
    for _ in 0..blocks {
        let block = mined(&chain, &wallet.address(), &[], ON_TIME);
        chain.add_block(block).unwrap();
        chain.keep_up(4);
    }
    chain
}

#[test]
fn a_full_store_drops_its_oldest_blocks_and_keeps_going() {
    let drive = flash(STATE_SIZE * 2 + 128 * 1024);
    let chain = long_chain(&drive, 2_500, None);
    let held = chain.held();
    assert!(held.oldest > 1_000, "oldest {}", held.oldest);
    assert!(chain.height() - held.oldest > 288);
    assert_eq!(chain.best_chain_from(held.oldest).len() as u64, chain.height() - held.oldest + 1);

    // A new node asks from the first block and hears plainly that it has gone.
    let served = chain.blocks_after(&[(0, chain.genesis_id())], 8);
    assert_eq!(served, Served::Pruned { oldest: held.oldest });

    // Blocks made here are the same as anyone's: a node with no history follows them from a fresh
    // start only through the blocks it is given, and the tip stays reachable from marks.
    let locator = chain.locator();
    assert_eq!(locator.last().map(|(height, _)| *height), Some(0));
    assert!(locator.iter().all(|(height, id)| chain.id_at(*height) == Some(*id)));

    let (tip, hash) = (chain.tip(), chain.unspent_hash());
    let back = powered_again(&drive);
    let again = Chain::open(coin(), plugins(), keep_on(&back, None));
    assert_eq!(again.tip(), tip);
    assert_eq!(again.unspent_hash(), hash);
    assert!(matches!(again.opened(), Opened::Resumed { read } if read < 400));
}

#[test]
fn the_number_of_recent_blocks_decides_what_stays() {
    let drive = flash(1024 * 1024);
    let chain = long_chain(&drive, 1_400, Some(300));
    let held = chain.held();
    assert!(chain.height() - held.oldest >= 300);
    assert!(chain.height() - held.oldest <= 300 + 512 + 256, "oldest {}", held.oldest);
}

#[test]
fn a_reorganisation_below_the_held_blocks_is_refused_and_harms_nothing() {
    let drive = flash(STATE_SIZE * 2 + 128 * 1024);
    let rival = Wallet::from_secret([8; 32]).unwrap();
    let mut chain = long_chain(&drive, 20, None);
    let mut other = Chain::new(coin(), plugins());
    feed(&mut other, &chain.best_chain_from(1));

    // The rival mines apart from block 20. This node goes on far past what its store can hold.
    let mut branch = Vec::new();
    for _ in 0..2_600 {
        let block = mined(&other, &rival.address(), &[], ON_TIME);
        other.add_block(block.clone()).unwrap();
        branch.push(block);
    }
    let wallet = Wallet::from_secret([9; 32]).unwrap();
    for _ in 0..2_500 {
        let block = mined(&chain, &wallet.address(), &[], ON_TIME);
        chain.add_block(block).unwrap();
        chain.keep_up(4);
    }
    assert!(chain.oldest_held() > 21);
    let (tip, hash) = (chain.tip(), chain.unspent_hash());

    // Its blocks come from where the chains part, and they connect to nothing held here.
    let mut outcomes = Vec::new();
    for block in &branch {
        outcomes.push(chain.add_block(block.clone()));
    }
    assert!(outcomes.iter().all(|outcome| !matches!(outcome, Ok(AddOutcome::Reorganized { .. }))));
    assert_eq!(chain.tip(), tip);
    assert_eq!(chain.unspent_hash(), hash);
}

#[test]
fn a_damaged_state_falls_back_and_says_so() {
    let drive = flash(1024 * 1024);
    let chain = long_chain(&drive, 900, None);
    let (tip, hash) = (chain.tip(), chain.unspent_hash());
    drop(chain);

    // The newest place is damaged: the older one and the log bring everything back.
    let one = powered_again(&drive);
    let newest = (0..2)
        .max_by_key(|place| {
            let start = place * STATE_SIZE + 4;
            let bytes = &one.lock().unwrap().bytes;
            u64::from_le_bytes(bytes[start..start + 8].try_into().unwrap())
        })
        .unwrap();
    one.lock().unwrap().bytes[newest * STATE_SIZE + 5_000] ^= 0x40;
    let back = Chain::open(coin(), plugins(), keep_on(&one, None));
    assert_eq!((back.tip(), back.unspent_hash()), (tip, hash));

    // Both are damaged: the blocks held here give the chain back, and the node says so.
    let two = powered_again(&drive);
    for place in 0..2 {
        two.lock().unwrap().bytes[place * STATE_SIZE + 5_000] ^= 0x40;
    }
    let back = Chain::open(coin(), plugins(), keep_on(&two, None));
    assert_eq!((back.tip(), back.unspent_hash()), (tip, hash));
    assert!(matches!(back.opened(), Opened::Rebuilt { .. }));
}

#[test]
fn a_damaged_state_and_no_first_blocks_starts_over_and_says_so() {
    let drive = flash(STATE_SIZE * 2 + 128 * 1024);
    let chain = long_chain(&drive, 2_500, None);
    drop(chain);
    let two = powered_again(&drive);
    for place in 0..2 {
        two.lock().unwrap().bytes[place * STATE_SIZE + 5_000] ^= 0x40;
    }
    let mut back = Chain::open(coin(), plugins(), keep_on(&two, None));
    assert_eq!(back.opened(), Opened::Lost);
    assert_eq!(back.height(), 0);
    let wallet = Wallet::from_secret([9; 32]).unwrap();
    let block = mined(&back, &wallet.address(), &[], 0);
    assert_eq!(back.add_block(block), Ok(AddOutcome::NewTip));
    let reopened = Chain::open(coin(), plugins(), keep_on(&two, None));
    assert_eq!(reopened.height(), 1);
}

#[test]
fn a_store_for_another_coin_reads_as_empty() {
    let drive = flash(1024 * 1024);
    let chain = long_chain(&drive, 50, None);
    drop(chain);
    let mut other = coin();
    other.message = "another".into();
    let chain = Chain::open(other, plugins(), keep_on(&drive, None));
    assert_eq!(chain.height(), 0);
}

#[test]
fn data_outputs_never_enter_the_unspent_set() {
    let wallet = Wallet::from_secret([9; 32]).unwrap();
    let mut chain = Chain::new(coin(), plugins());
    for _ in 0..4 {
        let block = mined(&chain, &wallet.address(), &[], 0);
        chain.add_block(block).unwrap();
    }
    let before = chain.unspent_count();
    let note = wallet.note(&chain, b"hello chain", 0, &[]).unwrap();
    assert!(note.outputs.iter().any(|output| output.is_data()));
    let block = mined(&chain, &wallet.address(), std::slice::from_ref(&note), 0);
    chain.add_block(block).unwrap();
    // One coin spent, its change and the coinbase made. The data output is not counted.
    assert_eq!(chain.unspent_count(), before + 1);
    let data_index = note.outputs.iter().position(|o| o.is_data()).unwrap() as u32;
    let point = olc_core::OutPoint {
        txid: note.id(),
        index: data_index,
    };
    assert!(!chain.is_unspent(&point));

    let mut noted = chain.block_template_with(&wallet.address(), &[], 1 << 44, b"mined by me");
    assert!(noted.txs[0].outputs[1].is_data());
    assert!(olc_core::Consensus::try_seal(&olc_pow::ProofOfWork, &mut noted.header, 1 << 20));
    let count = chain.unspent_count();
    chain.add_block(noted).unwrap();
    assert_eq!(chain.unspent_count(), count + 1);
}

#[test]
fn a_new_coin_keeps_its_message_in_the_genesis_block() {
    let mut config = coin();
    config.message = "our own coin".into();
    config.message_in_block = true;
    let genesis = config.genesis();
    let data = genesis.txs[0].outputs.iter().find(|out| out.is_data()).unwrap();
    assert_eq!(data.lock, b"our own coin");
    assert_eq!(Chain::new(config, plugins()).unspent_count(), 0);
}

#[test]
fn the_default_coin_stays_on_its_chain() {
    let id = olc_core::CoinConfig::olc().genesis().id();
    let bytes: Vec<String> = id.iter().take(4).map(|byte| format!("{byte:02x}")).collect();
    assert_eq!(bytes.join(""), "82816fe4");
}

