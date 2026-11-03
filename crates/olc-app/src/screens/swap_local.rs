//! A swap between two coins of the OLC family, both made on this computer, with no node anywhere
//! else. The maker's offer names its coin by id, the taker's node fetches that coin from the
//! maker's node, and nobody types an address. The pictures show the short offer and its code, a
//! lock with coin names and times, the finished swaps folded away, and a refusal in Korean.
use super::live::{live_root, tell};
use super::live_swap::{fill, has, new_swap, press, press_till, save, stage_of, start, swap_of, unix_ms, wait_in};
use super::Window;
use crate::core::swap::{Outcome, Stage};
use crate::core::{save_coin, Command, Personal};
use crate::settings::Paths;
use egui::accesskit::Role;
use olc_core::coin::{CoinConfig, COIN};
use olc_store::coin_file::{new_coin, NewCoin};

const MAKER_PORT: u16 = 7860;
const TAKER_PORT: u16 = 7870;

fn coin(name: &str, ticker: &str) -> CoinConfig {
    let values = vec![
        ("name".into(), name.into()),
        ("ticker".into(), ticker.into()),
        ("rules.block_time_ms".into(), "10000".into()),
        ("genesis_time_ms".into(), unix_ms().to_string()),
    ];
    new_coin(&NewCoin { values }).unwrap()
}

fn runs(window: &mut Window, ticker: &str) {
    wait_in(window, &format!("{ticker} runs"), 2, |snap| snap.node.coin.as_ref().is_some_and(|coin| coin.ticker == ticker));
}

fn order(window: &mut Window, personal: Personal, key: &str) {
    window.state().send(Command::Own(personal));
    wait_in(window, key, 1, |snap| snap.notice.as_ref().is_some_and(|notice| notice.key == key));
}

#[test]
#[ignore = "mines two coins for minutes: run it by name"]
fn local_swap_between_two_coins() {
    let (swt, two) = (coin("Swap Test", "SWT"), coin("Second Coin", "TWO"));
    let (maker_root, taker_root) = (live_root("LM"), live_root("LT"));
    // The maker ran the coin it asks for before, as anyone who offers for a coin would have.
    save_coin(&Paths { root: maker_root.clone() }, &two).unwrap();
    let mut m = start(&maker_root, MAKER_PORT, Vec::new(), 1_000);
    let mut t = start(&taker_root, TAKER_PORT, Vec::new(), 1_000);
    m.state().send(Command::Join { coin: Box::new(swt), address: None });
    t.state().send(Command::Join { coin: Box::new(two), address: None });
    runs(&mut m, "SWT");
    runs(&mut t, "TWO");
    m.state().send(Command::SetMining(true));
    t.state().send(Command::SetMining(true));
    wait_in(&mut m, "SWT to spend", 10, |snap| snap.node.spendable >= 110 * COIN);
    wait_in(&mut t, "TWO to spend", 10, |snap| snap.node.spendable >= 30 * COIN);
    wait_in(&mut m, "TWO among the coins", 1, |snap| snap.swaps.coins.iter().any(|(_, ticker)| ticker == "TWO"));

    press(&mut m, "TWO");
    fill(&mut m, Role::TextInput, "주는 양 (SWT)", "100");
    fill(&mut m, Role::TextInput, "받는 양 (TWO)", "20");
    press_till(&mut m, "제안 만들기", |window| !window.state().snapshot().swaps.swaps.is_empty());
    let first = new_swap(&m, &[]).unwrap();
    let offer = first.text.clone().unwrap();
    tell(&format!("  offer text: {} letters: {offer}", offer.len()));
    save(&mut m, "local-01-short-offer");
    press_till(&mut m, "QR 코드로 보기", |window| has(window, "QR 코드 숨기기"));
    save(&mut m, "local-02-offer-qr");
    press_till(&mut m, "QR 코드 숨기기", |window| has(window, "QR 코드로 보기"));

    // The taker never ran SWT. Its node asks the maker's node for it.
    fill(&mut t, Role::MultilineTextInput, "받은 제안을 여기에 붙여 넣으세요", &offer);
    save(&mut t, "local-03-taker-reads-offer");
    press_till(&mut t, "받아들이기", |window| swap_of(&window.state().snapshot(), &first.id).is_some());
    let answer = swap_of(&t.state().snapshot(), &first.id).and_then(|swap| swap.text).unwrap();
    tell(&format!("  answer text: {} letters", answer.len()));
    fill(&mut m, Role::MultilineTextInput, "상대가 보낸 답을 여기에 붙여 넣으세요", &answer);
    press_till(&mut m, "교환 시작", |window| stage_of(window, &first.id) != Some(Stage::WaitingForAnswer));
    wait_in(&mut m, "the maker locks", 3, |snap| swap_of(snap, &first.id).is_some_and(|swap| swap.stage == Stage::Locked));
    let following = |snap: &crate::core::Snapshot| snap.swaps.other.as_ref().is_some_and(|(_, height, peers)| *peers > 0 && *height > 0);
    wait_in(&mut m, "the maker follows TWO", 2, following);
    save(&mut m, "local-04-maker-lock-card");
    wait_in(&mut t, "the taker locks", 10, |snap| swap_of(snap, &first.id).is_some_and(|swap| swap.stage == Stage::Locked));
    save(&mut t, "local-05-taker-lock-card");

    for (window, side) in [(&mut m, "maker"), (&mut t, "taker")] {
        let what = format!("the {side}'s swap ends");
        wait_in(window, &what, 20, |snap| swap_of(snap, &first.id).is_some_and(|swap| matches!(swap.stage, Stage::Ended(_))));
        assert_eq!(stage_of(window, &first.id), Some(Stage::Ended(Outcome::Swapped)));
    }
    save(&mut m, "local-06-finished-folded");
    press_till(&mut m, "끝난 교환 1개 보기", |window| has(window, "끝난 교환 숨기기"));
    save(&mut m, "local-07-finished-shown");
    let snap = t.state().snapshot();
    tell(&format!("  taker: {} TWO spendable, height {}", snap.node.spendable / COIN, snap.node.height));

    // A payment sent twice: the second time its coins are spent, and the node says so.
    let (unsigned, signed) = (taker_root.join("again.unsigned.json"), taker_root.join("again.signed.json"));
    let (unsigned, signed) = (unsigned.display().to_string(), signed.display().to_string());
    let to = t.state().snapshot().node.address.clone();
    order(&mut t, Personal::Watch(to.clone()), "notice.saved");
    order(&mut t, Personal::Prepare { to, units: COIN, fee: 1_000, save: Some(unsigned.clone()) }, "notice.prepared");
    order(&mut t, Personal::Sign { path: unsigned, save: Some(signed.clone()) }, "notice.signed");
    order(&mut t, Personal::Broadcast(signed.clone()), "notice.broadcast");
    let sent_at = t.state().snapshot().node.height;
    wait_in(&mut t, "the payment in a block", 5, |snap| snap.node.height >= sent_at + 3);
    t.state().send(Command::Own(Personal::Broadcast(signed)));
    wait_in(&mut t, "the refusal", 1, |snap| snap.notice.as_ref().is_some_and(|notice| notice.key == "notice.refused_missing_input"));
    save(&mut t, "local-08-refusal-in-korean");
    tell(&format!("done: {}", first.id));
}
