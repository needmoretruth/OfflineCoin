//! A node that holds headers only, against real full nodes and real blocks.
mod common;
mod store;

use common::{deliver, engine_of, first_olc, mine, silence, test_coin};
use olc_core::coin::COIN;
use olc_core::proof::{merkle_branch, root_of_branch};
use olc_core::types::merkle_root;
use olc_core::{
    Block, BlockHeader, Chain, CoinConfig, CoinProof, HeaderChain, HeaderRefused, OutPoint, Shelf,
    Tx, TxOut,
};
use olc_node::Engine;
use olc_wallet::Wallet;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use store::{flash, keep_on, plugins, STATE_SIZE};

const EVERYTHING: usize = usize::MAX;

fn real_blocks() -> Vec<Block> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/olc-blocks.bin");
    let data = std::fs::read(path).unwrap();
    let mut blocks = Vec::new();
    let mut rest = data.as_slice();
    while let Some((size, after)) = rest.split_first_chunk::<4>() {
        let size = u32::from_le_bytes(*size) as usize;
        blocks.push(postcard::from_bytes(&after[..size]).unwrap());
        rest = &after[size..];
    }
    blocks
}

fn light_of(seed: u8, coin: CoinConfig) -> Engine {
    let mut node = engine_of(seed, coin);
    node.become_light(EVERYTHING, &[]);
    node
}

/// Lets time pass until the nodes have nothing left to say.
fn settle(nodes: &mut [Engine], clock: &mut u64) {
    for _ in 0..40 {
        *clock += 500;
        nodes.iter_mut().for_each(|node| node.tick(*clock));
        deliver(nodes);
    }
}

/// A block late enough to hold what waits in the pool.
fn confirm(node: &mut Engine, clock: &mut u64) {
    *clock += 6_000;
    mine(node, *clock);
}

fn mine_on(node: &mut Engine, count: usize, clock: &mut u64) {
    for _ in 0..count {
        *clock += 1_000;
        mine(node, *clock);
    }
}

#[test]
fn every_real_header_passes_as_it_did_for_a_full_node() {
    let coin = first_olc();
    let mut full = engine_of(1, coin.clone());
    for block in real_blocks() {
        // The file holds branches that lost too.
        let _ = full.chain_mut().add_block(block);
    }
    let chain = full.chain();
    let consensus = chain.plugins().consensus.as_ref();
    let mut headers = HeaderChain::new(coin.genesis().header, EVERYTHING);
    let best = chain.blocks_from(1, chain.height() as usize);
    for page in best.chunks(16) {
        let page: Vec<BlockHeader> = page.iter().map(|block| block.header).collect();
        headers.offer(&page, chain.config(), consensus).unwrap();
    }
    assert!(headers.height() > 2_000);
    assert_eq!(headers.height(), chain.height());
    assert_eq!(headers.tip().id(), chain.tip());
    assert_eq!(headers.work(), chain.total_work());
    let bytes = headers.held() * size_of::<BlockHeader>();
    println!("{} headers, {bytes} bytes", headers.held());
}

#[test]
fn a_header_that_breaks_a_rule_is_refused() {
    let blocks = real_blocks();
    let coin = first_olc();
    let full = engine_of(1, coin.clone());
    let chain = full.chain();
    let consensus = chain.plugins().consensus.as_ref();
    let start = || HeaderChain::new(coin.genesis().header, EVERYTHING);
    let first = blocks[0].header;
    let offer = |header: BlockHeader| start().offer(&[header], chain.config(), consensus);

    assert!(offer(first).is_ok());
    let bad_seal = BlockHeader { nonce: first.nonce.wrapping_add(1), ..first };
    assert_eq!(offer(bad_seal), Err(HeaderRefused::BadSeal));
    let elsewhere = BlockHeader { previous: [7; 32], ..first };
    assert_eq!(offer(elsewhere), Err(HeaderRefused::NoParent));
    let early = BlockHeader { time_ms: 0, ..first };
    assert!(offer(early).is_err());
    let easy = BlockHeader { difficulty: 1, ..first };
    assert!(offer(easy).is_err());
}

#[test]
fn a_branch_climbs_to_the_root_from_every_place_in_a_tree() {
    for count in 1..40u32 {
        let tx = |height: u32| Tx {
            version: 1,
            inputs: Vec::new(),
            outputs: Vec::new(),
            coinbase_height: Some(height.into()),
            lock_time: 0,
        };
        let txs: Vec<Tx> = (0..count).map(tx).collect();
        let root = merkle_root(&txs);
        for position in 0..count as usize {
            let branch = merkle_branch(&txs, position);
            assert_eq!(root_of_branch(txs[position].id(), position as u32, &branch), root);
        }
    }
}

/// A real block mined by `node`, with one more payment in it than the coinbase.
fn block_with_a_payment(node: &mut Engine, clock: &mut u64, to: &Engine) -> Block {
    mine_on(node, 3, clock);
    node.pay(to.address(), COIN, node.chain().rules().min_fee).unwrap();
    confirm(node, clock);
    let height = node.chain().height();
    node.chain().blocks_from(height, 1).pop().unwrap()
}

#[test]
fn a_forged_proof_is_refused() {
    let mut clock = 1_000;
    let mut miner = engine_of(1, test_coin());
    let payee = engine_of(2, test_coin());
    let block = block_with_a_payment(&mut miner, &mut clock, &payee);
    let height = miner.chain().height();
    let paid = block.txs[1].id();
    let point = OutPoint { txid: paid, index: 0 };
    let honest = CoinProof::of(&block, height, &point).unwrap();
    let header = block.header;
    let owner = payee.address();
    assert!(honest.check(&header, &owner).is_some());

    // The same proof read for somebody else's address.
    assert!(honest.check(&header, &miner.address()).is_none());
    // Under a header it does not belong to.
    let other = BlockHeader { merkle_root: [9; 32], ..header };
    assert!(honest.check(&other, &owner).is_none());
    // A made-up transaction paying the owner, with the real branch.
    let mut made_up = honest.clone();
    made_up.tx.outputs[0].value = 1_000 * COIN;
    assert!(made_up.check(&header, &owner).is_none());
    // A wrong place in the tree, a branch cut short, and one with a hash changed.
    let mut moved = honest.clone();
    moved.position ^= 1;
    assert!(moved.check(&header, &owner).is_none());
    let mut past_the_end = honest.clone();
    past_the_end.position += 1 << past_the_end.branch.len();
    assert!(past_the_end.check(&header, &owner).is_none());
    let mut short = honest.clone();
    short.branch.pop();
    assert!(short.check(&header, &owner).is_none());
    let mut changed = honest.clone();
    changed.branch[0][0] ^= 1;
    assert!(changed.check(&header, &owner).is_none());
    // An output that does not exist.
    let mut missing = honest.clone();
    missing.output = 40;
    assert!(missing.check(&header, &owner).is_none());
}

#[test]
fn a_data_output_holding_an_address_is_no_coin() {
    let owner = engine_of(2, test_coin()).address();
    let note = TxOut::data(owner.to_vec());
    let tx = Tx { version: 1, inputs: Vec::new(), outputs: vec![note], coinbase_height: Some(1), lock_time: 0 };
    let header = BlockHeader {
        version: 1,
        previous: [0; 32],
        merkle_root: tx.id(),
        time_ms: 0,
        difficulty: 1,
        nonce: 0,
    };
    let proof = CoinProof { height: 1, tx, output: 0, position: 0, branch: Vec::new() };
    assert!(proof.check(&header, &owner).is_none());
}

#[test]
fn a_light_node_follows_the_chain_and_knows_its_balance_from_proofs() {
    let mut clock = 1_000;
    let mut miner = engine_of(1, test_coin());
    mine_on(&mut miner, 5, &mut clock);
    let mut nodes = [miner, light_of(2, test_coin())];
    settle(&mut nodes, &mut clock);
    assert_eq!(nodes[1].status().height, 5);
    assert_eq!(nodes[1].status().tip, nodes[0].chain().tip());
    assert_eq!(nodes[1].status().work, nodes[0].chain().total_work());
    assert_eq!(nodes[1].status().balance, 0);

    let to = nodes[1].address();
    let fee = nodes[0].chain().rules().min_fee;
    nodes[0].pay(to, 3 * COIN, fee).unwrap();
    deliver(&mut nodes);
    confirm(&mut nodes[0], &mut clock);
    settle(&mut nodes, &mut clock);
    assert_eq!(nodes[1].status().height, 6);
    assert_eq!(nodes[1].status().balance, 3 * COIN);
    assert_eq!(nodes[1].status().balance, nodes[0].chain().balance(&to));
}

#[test]
fn a_light_node_pays_a_full_node_and_sees_its_change() {
    let mut clock = 1_000;
    let mut miner = engine_of(1, test_coin());
    mine_on(&mut miner, 5, &mut clock);
    let mut nodes = [miner, light_of(2, test_coin()), engine_of(3, test_coin())];
    settle(&mut nodes, &mut clock);
    let (light, other) = (nodes[1].address(), nodes[2].address());
    let fee = nodes[0].chain().rules().min_fee;
    nodes[0].pay(light, 5 * COIN, fee).unwrap();
    deliver(&mut nodes);
    confirm(&mut nodes[0], &mut clock);
    settle(&mut nodes, &mut clock);

    nodes[1].pay(other, 2 * COIN, fee).unwrap();
    deliver(&mut nodes);
    confirm(&mut nodes[0], &mut clock);
    settle(&mut nodes, &mut clock);
    assert_eq!(nodes[2].chain().balance(&other), 2 * COIN);
    assert_eq!(nodes[1].status().balance, 3 * COIN - fee);
    assert_eq!(nodes[1].status().balance, nodes[0].chain().balance(&light));
}

#[test]
fn a_light_node_follows_a_reorganisation_and_drops_the_coin_that_left() {
    let mut clock = 1_000;
    let mut one = engine_of(1, test_coin());
    let mut two = engine_of(3, test_coin());
    mine_on(&mut one, 5, &mut clock);
    for block in one.chain().blocks_from(1, 5) {
        two.chain_mut().add_block(block).unwrap();
    }
    let mut pair = [one, light_of(2, test_coin())];
    settle(&mut pair, &mut clock);
    let light_address = pair[1].address();
    let fee = pair[0].chain().rules().min_fee;
    pair[0].pay(light_address, COIN, fee).unwrap();
    deliver(&mut pair);
    confirm(&mut pair[0], &mut clock);
    settle(&mut pair, &mut clock);
    assert_eq!(pair[1].status().balance, COIN);

    // A longer branch from the other miner, which never saw that payment.
    mine_on(&mut two, 4, &mut clock);
    let [one, light] = pair;
    let mut nodes = [two, light, one];
    settle(&mut nodes, &mut clock);
    assert_eq!(nodes[1].status().height, 9);
    assert_eq!(nodes[1].status().tip, nodes[0].chain().tip());
    assert_eq!(nodes[1].light().unwrap().status().reorganized, 1);
    assert_eq!(nodes[1].status().balance, 0);
}

#[test]
fn a_reorganisation_longer_than_one_page_still_lands() {
    let mut clock = 1_000;
    let mut one = engine_of(1, test_coin());
    let mut two = engine_of(3, test_coin());
    mine_on(&mut one, 30, &mut clock);
    mine_on(&mut two, 40, &mut clock);
    let mut pair = [one, light_of(2, test_coin())];
    settle(&mut pair, &mut clock);
    assert_eq!(pair[1].status().height, 30);
    let [_, light] = pair;
    let mut nodes = [two, light];
    settle(&mut nodes, &mut clock);
    assert_eq!(nodes[1].status().height, 40);
    assert_eq!(nodes[1].status().tip, nodes[0].chain().tip());
}

/// Headers by height, as a computer's file holds them.
#[derive(Clone, Default)]
struct Shelved(Arc<Mutex<BTreeMap<u64, BlockHeader>>>);

impl Shelf for Shelved {
    fn put(&mut self, height: u64, header: &BlockHeader) -> bool {
        self.0.lock().unwrap().insert(height, *header);
        true
    }

    fn get(&self, height: u64) -> Option<BlockHeader> {
        self.0.lock().unwrap().get(&height).copied()
    }
}

/// A node whose small store has dropped its oldest blocks.
fn pruned_node(shelf: Option<Shelved>, clock: &mut u64) -> Engine {
    let drive = flash(STATE_SIZE * 2 + 128 * 1024);
    let mut keep = keep_on(&drive, None);
    keep.shelf = shelf.map(|shelf| Box::new(shelf) as Box<dyn Shelf>);
    let chain = Chain::open(test_coin(), plugins(), keep);
    let mut pruned = Engine::on_chain(chain, Wallet::from_secret([1; 32]).unwrap());
    pruned.set_merging(false);
    for _ in 0..6_000 {
        mine_on(&mut pruned, 1, clock);
        pruned.tick(*clock);
        silence(&mut pruned);
    }
    assert!(pruned.chain().oldest_held() > 1);
    pruned
}

#[test]
fn a_pruned_computer_still_serves_every_header() {
    let mut clock = 1_000;
    let pruned = pruned_node(Some(Shelved::default()), &mut clock);
    let tip = pruned.status().height;
    let id = pruned.chain().id_at(tip);
    let mut nodes = [pruned, light_of(2, test_coin())];
    while nodes[1].status().height < tip && clock < 10_000_000 {
        settle(&mut nodes, &mut clock);
        clock += 1_000;
        nodes[1].tick(clock);
    }
    let light = nodes[1].light().unwrap();
    assert_eq!(nodes[1].status().height, tip);
    assert_eq!(light.headers().tip().id(), id.unwrap());
    assert_eq!(light.status().pruned_from, None);
}

#[test]
fn a_shelf_fills_from_what_the_log_holds_and_follows_a_reorganisation() {
    let drive = flash(STATE_SIZE * 2 + 4 * 1024 * 1024);
    let chain = Chain::open(test_coin(), plugins(), keep_on(&drive, None));
    let mut node = Engine::on_chain(chain, Wallet::from_secret([1; 32]).unwrap());
    let mut clock = 1_000;
    mine_on(&mut node, 40, &mut clock);
    node.tick(clock);
    silence(&mut node);
    let saved = node.chain().tip();
    drop(node);

    let shelf = Shelved::default();
    let mut keep = keep_on(&drive, None);
    keep.shelf = Some(Box::new(shelf.clone()));
    let chain = Chain::open(test_coin(), plugins(), keep);
    assert_eq!(chain.tip(), saved);
    assert!(chain.keeps_every_header());
    let same = |chain: &Chain| {
        chain.blocks_from(1, EVERYTHING).iter().enumerate().all(|(index, block)| {
            shelf.get(index as u64 + 1) == Some(block.header)
        })
    };
    assert!(same(&chain));

    let mut node = Engine::on_chain(chain, Wallet::from_secret([1; 32]).unwrap());
    let mut rival = engine_of(9, test_coin());
    for block in node.chain().blocks_from(1, 35) {
        let _ = rival.chain_mut().add_block(block);
    }
    assert_eq!(rival.status().height, 35);
    mine_on(&mut rival, 12, &mut clock);
    for block in rival.chain().blocks_from(36, EVERYTHING) {
        let _ = node.chain_mut().add_block(block);
    }
    assert_eq!(node.status().height, 47);
    assert!(same(node.chain()));
}

/// When a shorter branch with more work wins, the tip steps back and the shelf, which only ever
/// puts a header over another, keeps the old branch's headers above the new tip. The chain must
/// not answer from them: the app's view read the old tip as still on the best chain, above the
/// tip, and its service stopped.
#[test]
fn no_height_above_the_tip_has_a_block_whatever_the_shelf_holds() {
    let shelf = Shelved::default();
    let drive = flash(STATE_SIZE * 2 + 4 * 1024 * 1024);
    let mut keep = keep_on(&drive, None);
    keep.shelf = Some(Box::new(shelf.clone()));
    let chain = Chain::open(test_coin(), plugins(), keep);
    let mut node = Engine::on_chain(chain, Wallet::from_secret([1; 32]).unwrap());
    let mut clock = 1_000;
    mine_on(&mut node, 40, &mut clock);
    let tip = node.chain().blocks_from(40, 1)[0].header;
    let mut left = shelf.clone();
    left.put(41, &tip);
    left.put(42, &tip);
    assert_eq!(node.chain().id_at(40), Some(tip.id()));
    assert_eq!(node.chain().id_at(41), None, "height 41 is above the tip");
    assert_eq!(node.chain().id_at(42), None, "height 42 is above the tip");
}

#[test]
fn a_pruned_node_says_when_it_no_longer_holds_what_is_asked() {
    let mut clock = 1_000;
    let pruned = pruned_node(None, &mut clock);
    let oldest = pruned.chain().oldest_held();
    let mut nodes = [pruned, light_of(2, test_coin())];
    settle(&mut nodes, &mut clock);
    assert_eq!(nodes[1].light().unwrap().status().pruned_from, Some(oldest));
    assert_eq!(nodes[1].status().height, 0);
}

#[test]
fn a_light_node_never_mines() {
    let mut light = light_of(2, test_coin());
    assert!(!light.mine(10_000, 1_000));
}

#[test]
fn an_address_can_be_followed_without_a_key() {
    let mut clock = 1_000;
    let mut miner = engine_of(1, test_coin());
    mine_on(&mut miner, 3, &mut clock);
    let followed = miner.address();
    let mut full = engine_of(2, test_coin());
    full.watch(followed);
    let mut light = light_of(3, test_coin());
    light.watch(followed);
    let mut nodes = [miner, full, light];
    settle(&mut nodes, &mut clock);
    let held = nodes[0].chain().balance(&followed);
    assert!(held > 0);
    assert_eq!(nodes[1].watched(), vec![(followed, held)]);
    assert_eq!(nodes[2].watched(), vec![(followed, held)]);
}

#[test]
fn a_key_that_never_touches_a_network_can_pay() {
    let mut clock = 1_000;
    let mut miner = engine_of(1, test_coin());
    mine_on(&mut miner, 5, &mut clock);
    let offline = Wallet::from_secret([1; 32]).unwrap();
    let mut online = engine_of(2, test_coin());
    online.watch(offline.address());
    let payee = engine_of(3, test_coin()).address();
    let mut nodes = [miner, online];
    settle(&mut nodes, &mut clock);

    let fee = nodes[1].chain().rules().min_fee;
    let unsigned = nodes[1].prepare(payee, COIN, fee).unwrap();
    let stranger = Wallet::from_secret([9; 32]).unwrap();
    assert!(stranger.sign(&unsigned).is_err());
    let signed = offline.sign(&unsigned).unwrap();
    nodes[1].send_signed(signed);
    deliver(&mut nodes);
    confirm(&mut nodes[0], &mut clock);
    settle(&mut nodes, &mut clock);
    assert_eq!(nodes[1].chain().balance(&payee), COIN);
}
