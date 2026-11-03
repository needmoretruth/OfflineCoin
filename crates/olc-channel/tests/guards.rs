//! What keeps a channel safe where a plain test would not look: a node that prunes old blocks, two
//! closes asked at once, coins held for an open, a fake channel on the map, the fee said before
//! paying. The old state here is again a file the program itself signed.
mod desk;

use desk::{block, open, open_with, seconds, Party, World, DELAY, FEE};
use olc_channel::{Channel, PayState, Phase};
use olc_core::chain::FEWEST_RECENT;
use std::fs;

const CAPACITY: u64 = 1_000_000;

fn channel<'a>(party: &'a Party, id: &olc_core::Hash) -> &'a Channel {
    party.node.channel(id).expect("the channel is known")
}

fn invoice(parties: &mut [Party], world: &mut World, payee: usize, amount: u64) -> String {
    parties[payee].with(world, |node, ground| node.make_invoice(amount, "test", ground)).unwrap()
}

fn pay(parties: &mut [Party], world: &mut World, payer: usize, text: &str) -> olc_core::Hash {
    parties[payer].with(world, |node, ground| node.pay(text, ground)).unwrap()
}

fn paid(party: &Party, hash: &olc_core::Hash) -> bool {
    party.node.payments().iter().any(|payment| payment.hash == *hash && payment.state == PayState::Paid)
}

fn channel_file(party: &Party) -> std::path::PathBuf {
    let files = fs::read_dir(&party.folder).unwrap().flatten().map(|entry| entry.path());
    files.filter(|path| path.file_name().is_some_and(|name| name.to_string_lossy().starts_with("channel-"))).last().unwrap()
}

#[test]
fn a_node_that_prunes_old_blocks_still_catches_an_old_state() {
    let mut world = World::new();
    world.chain.set_recent(Some(FEWEST_RECENT));
    let mut parties = vec![world.party("pruned", "a", 1), world.party("pruned", "b", 2)];
    // The wait is long enough that B can still answer when it comes back.
    let delay = u16::try_from(FEWEST_RECENT + 120).unwrap();
    let ab = open_with(&mut parties, &mut world, (0, 1), CAPACITY, delay);
    let file = channel_file(&parties[0]);
    let backup = fs::read(&file).unwrap();
    let text = invoice(&mut parties, &mut world, 1, 300_000);
    let hash = pay(&mut parties, &mut world, 0, &text);
    seconds(&mut parties, &mut world, 6);
    assert!(paid(&parties[0], &hash));

    // B is away while A sends the old state and the chain grows past what a pruned node keeps.
    parties[1].cut = true;
    fs::write(&file, backup).unwrap();
    parties[0].restart();
    parties[0].with(&mut world, |node, ground| node.close(&ab, true, ground)).unwrap();
    let mined = block(&mut parties, &mut world);
    assert_eq!(mined.len(), 1, "the old commitment is on the chain");
    for _ in 0..FEWEST_RECENT + 40 {
        block(&mut parties, &mut world);
    }
    let b_before = world.balance(&parties[1]);
    parties[1].cut = false;
    seconds(&mut parties, &mut world, 40);
    let penalty = channel(&parties[1], &ab).chain.penalty.expect("B found the old state and answered");
    block(&mut parties, &mut world);
    assert!(world.has(&penalty), "the penalty is on the chain");
    assert_eq!(world.balance(&parties[1]) - b_before, CAPACITY - FEE - FEE);
}

#[test]
fn two_closes_asked_at_once_end_in_one_closing_transaction() {
    let mut world = World::new();
    let mut parties = vec![world.party("both-close", "a", 1), world.party("both-close", "b", 2)];
    let ab = open(&mut parties, &mut world, 0, 1, CAPACITY);
    let text = invoice(&mut parties, &mut world, 1, 100_000);
    let hash = pay(&mut parties, &mut world, 0, &text);
    seconds(&mut parties, &mut world, 6);
    assert!(paid(&parties[0], &hash));

    // Both ask before either hears the other.
    let (a_before, b_before) = (world.balance(&parties[0]), world.balance(&parties[1]));
    parties[0].with(&mut world, |node, ground| node.close(&ab, false, ground)).unwrap();
    parties[1].with(&mut world, |node, ground| node.close(&ab, false, ground)).unwrap();
    seconds(&mut parties, &mut world, 6);
    let a_close = channel(&parties[0], &ab).closing_tx.clone().expect("A holds a close both signed");
    let b_close = channel(&parties[1], &ab).closing_tx.clone().expect("B holds a close both signed");
    assert_eq!(a_close.id(), b_close.id(), "one closing transaction");
    block(&mut parties, &mut world);
    block(&mut parties, &mut world);
    assert!(world.has(&a_close.id()));
    assert_eq!(world.balance(&parties[0]) - a_before, CAPACITY - 100_000 - FEE);
    assert_eq!(world.balance(&parties[1]) - b_before, 100_000);
    assert_eq!(channel(&parties[0], &ab).phase, Phase::Closed);
    assert_eq!(channel(&parties[1], &ab).phase, Phase::Closed);
}

fn view_of(party: &Party, world: &World, id: &olc_core::Hash) -> olc_channel::ChannelView {
    party.node.views(world.chain.height()).into_iter().find(|view| view.id == *id).expect("the channel is shown")
}

fn sorted(mut points: Vec<olc_core::OutPoint>) -> Vec<olc_core::OutPoint> {
    points.sort();
    points
}

#[test]
fn the_coins_for_a_channel_stay_held_from_the_ask_until_it_opens() {
    let mut world = World::new();
    let mut parties = vec![world.party("held", "a", 1), world.party("held", "b", 2)];
    // B hears nothing yet, so the channel waits at the ask.
    parties[1].cut = true;
    let peer = parties[1].id();
    let id = parties[0].with(&mut world, |node, ground| node.open_channel(peer, CAPACITY, DELAY, ground)).unwrap();
    let held = sorted(parties[0].node.held_coins());
    let value: u64 = held.iter().filter_map(|point| world.chain.coin_at(point)).map(|coin| coin.output.value).sum();
    assert!(value >= CAPACITY + FEE, "the held coins pay for the channel");
    parties[0].restart();
    assert_eq!(sorted(parties[0].node.held_coins()), held, "a restart holds the same coins");

    parties[1].cut = false;
    seconds(&mut parties, &mut world, 8);
    for _ in 0..4 {
        block(&mut parties, &mut world);
    }
    assert_eq!(channel(&parties[0], &id).phase, Phase::Open);
    let funding = channel(&parties[0], &id).funding_tx.clone().expect("A funded the channel");
    let spent = sorted(funding.inputs.iter().map(|input| input.previous).collect());
    assert_eq!(spent, held, "the funding spends exactly the held coins");
    assert!(parties[0].node.held_coins().is_empty(), "let go once the channel is open");
}

#[test]
fn a_request_says_who_it_pays_and_the_fee_before_paying() {
    let mut world = World::new();
    let mut parties = vec![world.party("quote", "a", 1), world.party("quote", "b", 2), world.party("quote", "c", 3)];
    // A and C are out of each other's reach: all A knows of C comes through B.
    let (a, c) = (parties[0].id(), parties[2].id());
    parties[0].apart.push(c);
    parties[2].apart.push(a);
    let ab = open(&mut parties, &mut world, 0, 1, CAPACITY);
    open(&mut parties, &mut world, 1, 2, CAPACITY);
    seconds(&mut parties, &mut world, 3);
    let funding = channel(&parties[0], &ab).funding();
    let heard = |party: &Party| party.node.graph().edges().any(|edge| edge.announcement.funding == funding);
    assert!(heard(&parties[2]), "C heard of A and B's channel, signed by all four keys");
    assert!(parties[0].node.peers().iter().all(|(node, _)| *node != c), "A never heard C itself");

    let text = invoice(&mut parties, &mut world, 2, 50_000);
    let quote = parties[0].with(&mut world, |node, ground| node.quote(&text, ground)).unwrap();
    assert_eq!((quote.payee, quote.name.as_str(), quote.amount), (c, "c", 50_000), "C's name came with its own signature");
    assert!(quote.fee > 0, "B asks a fee to pass it on");
    let hash = pay(&mut parties, &mut world, 0, &text);
    seconds(&mut parties, &mut world, 10);
    assert!(paid(&parties[0], &hash));
    assert_eq!(parties[0].node.payments()[0].fee, quote.fee, "the fee paid is the fee said");

    // Once A and B's channel is closed on the chain, C forgets it.
    parties[0].with(&mut world, |node, ground| node.close(&ab, false, ground)).unwrap();
    seconds(&mut parties, &mut world, 3);
    block(&mut parties, &mut world);
    seconds(&mut parties, &mut world, 1);
    assert!(!heard(&parties[2]), "a spent funding output takes the channel off the map");
}

#[test]
fn coins_on_their_way_back_show_until_they_are_back() {
    let mut world = World::new();
    let mut parties = vec![world.party("coming", "a", 1), world.party("coming", "b", 2)];
    let ab = open(&mut parties, &mut world, 0, 1, CAPACITY);
    let text = invoice(&mut parties, &mut world, 1, 200_000);
    let hash = pay(&mut parties, &mut world, 0, &text);
    seconds(&mut parties, &mut world, 6);
    assert!(paid(&parties[0], &hash));

    parties[0].with(&mut world, |node, ground| node.close(&ab, true, ground)).unwrap();
    block(&mut parties, &mut world);
    let closed_at = world.chain.height();
    let waiting = view_of(&parties[0], &world, &ab);
    assert_eq!(waiting.phase, Phase::ForceClosing);
    assert_eq!((waiting.coming, waiting.back), (CAPACITY - 200_000, 0), "A's share is on its way back");
    assert_eq!(view_of(&parties[1], &world, &ab).back, 200_000, "B's share came back at once");
    for _ in 0..DELAY + 1 {
        block(&mut parties, &mut world);
    }
    seconds(&mut parties, &mut world, 2);
    let done = view_of(&parties[0], &world, &ab);
    assert_eq!(done.phase, Phase::Closed);
    assert_eq!((done.coming, done.back), (0, CAPACITY - 200_000 - 2 * FEE), "what came back, after the fees");

    // When and at which block it closed, and still so after a restart.
    assert_eq!(waiting.closed_at, None, "not closed while the share is on its way");
    assert_eq!(done.closed_at, Some(closed_at), "the block the close went into");
    let ended = done.ended_ms.expect("the moment it closed");
    assert!(ended > 0 && ended <= world.now_ms);
    parties[0].restart();
    let again = view_of(&parties[0], &world, &ab);
    assert_eq!((again.closed_at, again.ended_ms), (Some(closed_at), Some(ended)));
}

#[test]
fn an_open_nobody_answers_lets_its_coins_go() {
    let mut world = World::new();
    let mut parties = vec![world.party("silent", "a", 1), world.party("silent", "b", 2)];
    parties[1].cut = true;
    let peer = parties[1].id();
    let id = parties[0].with(&mut world, |node, ground| node.open_channel(peer, CAPACITY, DELAY, ground)).unwrap();
    seconds(&mut parties, &mut world, 2);
    assert!(!parties[0].node.held_coins().is_empty());
    world.now_ms += olc_channel::node::GIVE_UP_MS;
    seconds(&mut parties, &mut world, 1);
    assert_eq!(channel(&parties[0], &id).phase, Phase::Failed);
    assert!(parties[0].node.held_coins().is_empty(), "the coins go back to the wallet");
    let failed = view_of(&parties[0], &world, &id);
    assert_eq!((failed.ended_ms, failed.closed_at), (Some(world.now_ms), None), "when it gave up, and no block");
}
