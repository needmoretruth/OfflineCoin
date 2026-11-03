//! Channels between real nodes on a real chain: open, pay directly and through a node in the
//! middle, close together, close alone after the wait, lose everything for a revoked state, take a
//! payment back after its expiry, and carry on after a restart in the middle of a step. Every
//! state on the chain here was signed by the program itself; the cheat is an old file put back.
mod desk;

use desk::{block, deliver, open, second, seconds, Party, World, DELAY, FEE};
use olc_channel::{Channel, PayState, Phase};
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

#[test]
fn a_payment_crosses_a_node_in_the_middle_and_the_channels_close_together() {
    let mut world = World::new();
    let mut parties = vec![
        world.party("middle", "a", 1),
        world.party("middle", "b", 2),
        world.party("middle", "c", 3),
    ];
    let ab = open(&mut parties, &mut world, 0, 1, CAPACITY);
    let bc = open(&mut parties, &mut world, 1, 2, CAPACITY);
    seconds(&mut parties, &mut world, 3);

    // C asks; A pays through B, with B's fee on top.
    let text = invoice(&mut parties, &mut world, 2, 50_000);
    let hash = pay(&mut parties, &mut world, 0, &text);
    seconds(&mut parties, &mut world, 10);
    assert!(paid(&parties[0], &hash), "A sees the payment through");
    let fee = parties[0].node.payments()[0].fee;
    assert!(fee > 0, "B asked a fee");
    assert_eq!(channel(&parties[0], &ab).state.ours, CAPACITY - 50_000 - fee);
    assert_eq!(channel(&parties[1], &ab).state.ours, 50_000 + fee);
    assert_eq!(channel(&parties[1], &bc).state.ours, CAPACITY - 50_000);
    assert_eq!(channel(&parties[2], &bc).state.ours, 50_000);
    assert!(parties[2].node.invoices(0)[0].paid, "C's request shows as paid");
    assert!(channel(&parties[1], &ab).state.htlcs.is_empty() && channel(&parties[1], &bc).state.htlcs.is_empty());

    // A pays B directly too, a second time on the same channel.
    let text = invoice(&mut parties, &mut world, 1, 7_000);
    let direct = pay(&mut parties, &mut world, 0, &text);
    seconds(&mut parties, &mut world, 6);
    assert!(paid(&parties[0], &direct));
    assert_eq!(channel(&parties[0], &ab).state.ours, CAPACITY - 57_000 - fee);

    // A and B close together: each gets its share at once, the opener pays the fee.
    let (a_before, b_before) = (world.balance(&parties[0]), world.balance(&parties[1]));
    parties[0].with(&mut world, |node, ground| node.close(&ab, false, ground)).unwrap();
    seconds(&mut parties, &mut world, 3);
    let close = channel(&parties[0], &ab).closing_tx.clone().expect("a close both signed");
    assert_eq!(channel(&parties[1], &ab).closing_tx.as_ref().map(|tx| tx.id()), Some(close.id()));
    block(&mut parties, &mut world);
    assert!(world.has(&close.id()), "the close is on the chain");
    assert_eq!(world.balance(&parties[0]) - a_before, CAPACITY - 57_000 - fee - FEE);
    assert_eq!(world.balance(&parties[1]) - b_before, 57_000 + fee);
    block(&mut parties, &mut world);
    assert_eq!(channel(&parties[0], &ab).phase, Phase::Closed);
    assert_eq!(channel(&parties[1], &ab).phase, Phase::Closed);
}

#[test]
fn closing_alone_waits_out_the_delay_before_the_coins_come_back() {
    let mut world = World::new();
    let mut parties = vec![world.party("alone", "a", 1), world.party("alone", "b", 2)];
    let ab = open(&mut parties, &mut world, 0, 1, CAPACITY);
    let text = invoice(&mut parties, &mut world, 1, 100_000);
    let hash = pay(&mut parties, &mut world, 0, &text);
    seconds(&mut parties, &mut world, 6);
    assert!(paid(&parties[0], &hash));

    let (a_before, b_before) = (world.balance(&parties[0]), world.balance(&parties[1]));
    parties[0].with(&mut world, |node, ground| node.close(&ab, true, ground)).unwrap();
    block(&mut parties, &mut world);
    let close = channel(&parties[0], &ab).chain.seen.first().map(|(_, tx)| tx.id()).expect("the commitment is on the chain");
    assert_eq!(channel(&parties[0], &ab).phase, Phase::ForceClosing);
    // B's share pays its address at once; A's waits.
    assert_eq!(world.balance(&parties[1]) - b_before, 100_000);
    assert_eq!(world.balance(&parties[0]), a_before);
    for _ in 1..DELAY {
        assert!(channel(&parties[0], &ab).chain.claims.is_empty(), "nothing is taken before the wait is over");
        block(&mut parties, &mut world);
    }
    let claims = channel(&parties[0], &ab).chain.claims.clone();
    assert_eq!(claims.len(), 1, "A takes its coins when the wait is over");
    block(&mut parties, &mut world);
    assert!(world.has(&close) && world.has(&claims[0]));
    assert_eq!(world.balance(&parties[0]) - a_before, CAPACITY - 100_000 - 2 * FEE);
    seconds(&mut parties, &mut world, 2);
    assert_eq!(channel(&parties[0], &ab).phase, Phase::Closed);
}

#[test]
fn a_revoked_state_on_the_chain_hands_the_whole_channel_to_the_other_side() {
    let mut world = World::new();
    let mut parties = vec![world.party("cheat", "a", 1), world.party("cheat", "b", 2)];
    let ab = open(&mut parties, &mut world, 0, 1, CAPACITY);
    // A keeps a copy of its files while it still holds everything.
    let file = fs::read_dir(&parties[0].folder)
        .unwrap()
        .flatten()
        .map(|entry| entry.path())
        .find(|path| path.file_name().is_some_and(|name| name.to_string_lossy().starts_with("channel-")))
        .unwrap();
    let backup = fs::read(&file).unwrap();
    let text = invoice(&mut parties, &mut world, 1, 300_000);
    let hash = pay(&mut parties, &mut world, 0, &text);
    seconds(&mut parties, &mut world, 6);
    assert!(paid(&parties[0], &hash));

    // A puts the old file back and closes alone with the state where it had it all.
    fs::write(&file, backup).unwrap();
    parties[0].restart();
    assert_eq!(channel(&parties[0], &ab).state.ours, CAPACITY, "the old state is back");
    let b_before = world.balance(&parties[1]);
    parties[0].with(&mut world, |node, ground| node.close(&ab, true, ground)).unwrap();
    let mined = block(&mut parties, &mut world);
    assert_eq!(mined.len(), 1, "the old commitment is on the chain");
    let penalty = channel(&parties[1], &ab).chain.penalty.expect("B answered with the penalty");
    assert!(channel(&parties[1], &ab).chain.cheated());
    // What B's screens show beside the sentence that every coin comes to B.
    let height = world.chain.height();
    let shown = parties[1].node.views(height).into_iter().find(|view| view.id == ab).unwrap();
    assert_eq!((shown.ours, shown.theirs), (CAPACITY, 0));
    block(&mut parties, &mut world);
    assert!(world.has(&penalty));
    // B has all of it: A's share by the penalty, nothing to B's address from the old state.
    assert_eq!(world.balance(&parties[1]) - b_before, CAPACITY - FEE - FEE);
    for _ in 0..DELAY + 1 {
        block(&mut parties, &mut world);
    }
    assert!(channel(&parties[0], &ab).chain.claims.iter().all(|claim| !world.has(claim)), "A takes nothing back");
    assert_eq!(channel(&parties[1], &ab).phase, Phase::Closed);
}

#[test]
fn a_payment_nobody_takes_comes_back_after_its_expiry() {
    let mut world = World::new();
    let mut parties = vec![world.party("expiry", "a", 1), world.party("expiry", "b", 2)];
    let ab = open(&mut parties, &mut world, 0, 1, CAPACITY);
    let text = invoice(&mut parties, &mut world, 1, 40_000);
    let a_before = world.balance(&parties[0]);
    // B signs the payment in and then stops: it never ticks, so it never takes it.
    parties[0].with(&mut world, |node, ground| node.pay(&text, ground)).unwrap();
    deliver(&mut parties, &mut world);
    assert_eq!(channel(&parties[0], &ab).state.htlcs.len(), 1, "the payment waits in the channel");
    parties[1].cut = true;
    let expiry = u64::from(channel(&parties[0], &ab).state.htlcs[0].expiry);
    while world.chain.height() < expiry {
        block(&mut parties, &mut world);
    }
    // A goes to the chain at the expiry and takes the payment back through HTLC-timeout.
    assert_eq!(channel(&parties[0], &ab).phase, Phase::ForceClosing);
    for _ in 0..2 * DELAY + 4 {
        block(&mut parties, &mut world);
    }
    assert_eq!(channel(&parties[0], &ab).phase, Phase::Closed);
    let claims = channel(&parties[0], &ab).chain.claims.clone();
    assert!(claims.len() >= 3, "a timeout, then the two waits swept");
    assert!(claims.iter().all(|claim| world.has(claim)));
    // A has everything back but the fees: commitment, timeout, and two sweeps.
    assert_eq!(world.balance(&parties[0]) - a_before, CAPACITY - 4 * FEE);
    assert!(parties[0].node.payments().iter().all(|payment| payment.state == PayState::Failed));
}

#[test]
fn a_restart_in_the_middle_of_a_step_carries_on_from_the_files() {
    let mut world = World::new();
    let mut parties = vec![world.party("restart", "a", 1), world.party("restart", "b", 2)];
    let ab = open(&mut parties, &mut world, 0, 1, CAPACITY);
    let text = invoice(&mut parties, &mut world, 1, 20_000);
    let hash = pay(&mut parties, &mut world, 0, &text);
    // The offer leaves A; B signs; then both stop before anything else moves.
    let offer = parties[0].node.take_out().expect("A offered");
    if let olc_channel::Out::To(_, bytes) = offer {
        parties[1].with(&mut world, |node, ground| node.receive(&bytes, ground));
    }
    let _lost = parties[1].node.take_out().expect("B signed");
    parties[0].restart();
    parties[1].restart();
    assert!(channel(&parties[0], &ab).round.is_some(), "A still knows its turn");
    // What was lost is sent again; the turn ends and the payment goes through.
    seconds(&mut parties, &mut world, 12);
    assert!(paid(&parties[0], &hash));
    assert_eq!(channel(&parties[0], &ab).state.ours, CAPACITY - 20_000);
    assert_eq!(channel(&parties[1], &ab).state.ours, 20_000);
    // The key, the channel and the book with its secrets all went through the private writer.
    for party in &parties {
        let names: Vec<String> = fs::read_dir(&party.folder).unwrap().flatten().map(|entry| entry.file_name().to_string_lossy().into_owned()).collect();
        assert!(names.iter().any(|name| name == "node.key") && names.iter().any(|name| name == "book.json"), "{names:?}");
        assert!(names.iter().any(|name| name.starts_with("channel-")), "{names:?}");
        assert_eq!(desk::written_around(&party.folder), Vec::<std::path::PathBuf>::new());
    }
    let _ = second;
}
