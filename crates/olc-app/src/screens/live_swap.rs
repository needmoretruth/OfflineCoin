//! One coin swap and one way back, through two windows of the real app on the real coin. One
//! window runs OLC and pays OLC, the other runs a coin made with New coin and pays that. Every
//! step a person sees is saved at the window's size, in Korean and in basic mode.
use super::live::{live_folder, live_peer, live_root, live_root_kept, live_settings, tell};
use super::shots::shot;
use super::{app, harness, Window};
use crate::core::swap::{Outcome, Stage};
use crate::core::{Command, Snapshot, SwapView};
use egui::accesskit::Role;
use egui_kittest::kittest::{By, Queryable};
use olc_core::coin::COIN;
use olc_store::coin_file::{new_coin, NewCoin};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const SIZE: (u32, u32) = (1280, 1100);
const OLC_SIDE_PORT: u16 = 7880;
const NEW_SIDE_PORT: u16 = 7890;

pub(super) fn unix_ms() -> u128 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |since| since.as_millis())
}

fn shots() -> PathBuf {
    let folder = live_folder().join("swap-shots");
    std::fs::create_dir_all(&folder).unwrap();
    folder
}

/// Waits while the window keeps drawing, and stops at once on a swap notice that went wrong.
#[track_caller]
pub(super) fn wait_in(window: &mut Window, what: &str, minutes: u64, mut done: impl FnMut(&Snapshot) -> bool) {
    let started = Instant::now();
    loop {
        window.run_steps(1);
        let snap = window.state().snapshot();
        if let Some(notice) = snap.notice.as_ref().filter(|notice| !notice.good && notice.key.starts_with("swap.")) {
            panic!("{what}: {notice:?}");
        }
        if done(&snap) {
            break;
        }
        assert!(started.elapsed() < Duration::from_secs(minutes * 60), "waited {minutes} minutes for {what}");
        std::thread::sleep(Duration::from_millis(400));
    }
    tell(&format!("{what}: {} s", started.elapsed().as_secs()));
}

pub(super) fn swap_of(snap: &Snapshot, id: &str) -> Option<SwapView> {
    snap.swaps.swaps.iter().find(|swap| swap.id == id).cloned()
}

pub(super) fn stage_of(window: &Window, id: &str) -> Option<Stage> {
    swap_of(&window.state().snapshot(), id).map(|swap| swap.stage)
}

/// The swap that is new since `known`.
pub(super) fn new_swap(window: &Window, known: &[String]) -> Option<SwapView> {
    window.state().snapshot().swaps.swaps.iter().find(|swap| !known.contains(&swap.id)).cloned()
}

fn ids(window: &Window) -> Vec<String> {
    window.state().snapshot().swaps.swaps.iter().map(|swap| swap.id.clone()).collect()
}

pub(super) fn press(window: &mut Window, label: &str) {
    let button = window.query(By::new().role(Role::Button).label(label));
    let choice = || window.query(By::new().role(Role::Tab).label(label));
    button.or_else(choice).unwrap_or_else(|| panic!("no button {label}")).click();
    window.run_steps(3);
}

pub(super) fn has(window: &Window, label: &str) -> bool {
    window.query(By::new().role(Role::Button).label(label)).is_some()
}

/// Presses until `done` holds. A line that appears above a button between two frames gives the
/// button a new id, and a press sent to the old one goes nowhere, as a reader's press would.
pub(super) fn press_till(window: &mut Window, label: &str, done: impl Fn(&Window) -> bool) {
    for _ in 0..5 {
        if has(window, label) || window.query(By::new().role(Role::Tab).label(label)).is_some() {
            press(window, label);
        }
        for _ in 0..25 {
            window.run_steps(1);
            if done(window) {
                return;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    }
    panic!("pressing {label} did nothing");
}

/// Types into the field a person reads as `label`, as pasting would.
pub(super) fn fill(window: &mut Window, role: Role, label: &str, text: &str) {
    window.query(By::new().role(role).label(label)).unwrap_or_else(|| panic!("no field {label}")).focus();
    window.run_steps(2);
    window.query(By::new().role(role).label(label)).unwrap_or_else(|| panic!("no field {label}")).type_text(text);
    window.run_steps(3);
}

pub(super) fn save(window: &mut Window, name: &str) {
    shot(window, &shots(), name);
    tell(&format!("  shot {name}"));
}

pub(super) fn start(root: &Path, port: u16, peers: Vec<String>, cap: u64) -> Window {
    let mut settings = live_settings(port, &peers);
    settings.hash_cap = cap;
    settings.mine_on_start = true;
    let mut window = harness(app(root.to_path_buf(), settings, None), SIZE);
    window.state_mut().open("page.swap");
    window
}

#[test]
#[ignore = "needs the real coin: set OLC_LIVE to a running node"]
fn live_swap_and_a_way_back() {
    let Some(peer) = live_peer() else { return };
    let (olc_root, new_root) = (live_root_kept("O"), live_root("N"));
    let olc_other = format!("127.0.0.1:{}", NEW_SIDE_PORT);
    let new_other = format!("127.0.0.1:{}", OLC_SIDE_PORT + 1);
    // The window on OLC finds the new coin's node, and the other finds OLC, as two people on one
    // network would by looking around.
    let mut o = start(&olc_root, OLC_SIDE_PORT, vec![peer.clone(), olc_other], 2_000);
    let mut n = start(&new_root, NEW_SIDE_PORT, vec![peer.clone(), new_other.clone()], 1_000);
    wait_in(&mut n, "the new coin side starts on OLC", 2, |snap| snap.node.coin.as_ref().is_some_and(|coin| coin.ticker == "OLC"));
    std::thread::sleep(Duration::from_secs(3));
    let choices = NewCoin {
        values: vec![
            ("name".into(), "Swap Test".into()),
            ("ticker".into(), "SWT".into()),
            ("rules.block_time_ms".into(), "10000".into()),
            ("genesis_time_ms".into(), unix_ms().to_string()),
        ],
    };
    let coin = new_coin(&choices).unwrap();
    n.state().send(Command::Join { coin: Box::new(coin), address: None });
    wait_in(&mut n, "the new coin runs", 2, |snap| snap.node.coin.as_ref().is_some_and(|coin| coin.ticker == "SWT"));
    n.state().send(Command::SetMining(true));
    // Nobody types an address: the offer and its answer carry where each side's node listens.
    wait_in(&mut o, "OLC to spend", 30, |snap| snap.node.spendable >= 40 * COIN);
    o.state().send(Command::SetMining(false));
    wait_in(&mut n, "SWT to spend", 10, |snap| snap.node.spendable >= 250 * COIN);
    wait_in(&mut n, "OLC among the coins to swap for", 1, |snap| snap.swaps.coins.iter().any(|(_, ticker)| ticker == "OLC"));

    tell("swap 1");
    save(&mut n, "01-new-side-swap-page");
    fill(&mut n, Role::TextInput, "주는 양 (SWT)", "100");
    fill(&mut n, Role::TextInput, "받는 양 (OLC)", "20");
    save(&mut n, "02-new-side-offer-filled");
    press_till(&mut n, "제안 만들기", |window| !window.state().snapshot().swaps.swaps.is_empty());
    let first = new_swap(&n, &[]).unwrap();
    let offer = first.text.clone().unwrap();
    save(&mut n, "03-new-side-offer-made");
    press_till(&mut n, "QR 코드로 보기", |window| has(window, "QR 코드 숨기기"));
    save(&mut n, "04-new-side-offer-as-qr");
    press_till(&mut n, "QR 코드 숨기기", |window| has(window, "QR 코드로 보기"));

    save(&mut o, "05-olc-side-swap-page");
    fill(&mut o, Role::MultilineTextInput, "받은 제안을 여기에 붙여 넣으세요", &offer);
    save(&mut o, "06-olc-side-offer-pasted");
    press_till(&mut o, "받아들이기", |window| swap_of(&window.state().snapshot(), &first.id).is_some());
    let answer = swap_of(&o.state().snapshot(), &first.id).and_then(|swap| swap.text).unwrap();
    save(&mut o, "07-olc-side-offer-taken");

    fill(&mut n, Role::MultilineTextInput, "상대가 보낸 답을 여기에 붙여 넣으세요", &answer);
    save(&mut n, "08-new-side-answer-pasted");
    press_till(&mut n, "교환 시작", |window| stage_of(window, &first.id) != Some(Stage::WaitingForAnswer));
    wait_in(&mut n, "the new side locks", 3, |snap| swap_of(snap, &first.id).is_some_and(|swap| swap.stage == Stage::Locked));
    save(&mut n, "09-new-side-locked");
    wait_in(&mut o, "the OLC side locks", 10, |snap| swap_of(snap, &first.id).is_some_and(|swap| swap.stage == Stage::Locked));
    save(&mut o, "10-olc-side-locked");

    tell("swap 2, which the OLC side leaves");
    let known = ids(&n);
    fill(&mut n, Role::TextInput, "주는 양 (SWT)", "50");
    fill(&mut n, Role::TextInput, "받는 양 (OLC)", "10");
    press_till(&mut n, "제안 만들기", |window| window.state().snapshot().swaps.swaps.len() > known.len());
    let second = new_swap(&n, &known).unwrap();
    fill(&mut o, Role::MultilineTextInput, "받은 제안을 여기에 붙여 넣으세요", &second.text.clone().unwrap());
    press_till(&mut o, "받아들이기", |window| swap_of(&window.state().snapshot(), &second.id).is_some());
    let answer = swap_of(&o.state().snapshot(), &second.id).and_then(|swap| swap.text).unwrap();
    press_till(&mut o, "취소", |window| stage_of(window, &second.id) == Some(Stage::Ended(Outcome::Cancelled)));
    save(&mut o, "11-olc-side-left-the-second");
    fill(&mut n, Role::MultilineTextInput, "상대가 보낸 답을 여기에 붙여 넣으세요", &answer);
    press_till(&mut n, "교환 시작", |window| stage_of(window, &second.id) != Some(Stage::WaitingForAnswer));

    let first_done = |window: &mut Window, side: &str, at: &str| {
        let what = format!("the {side} side takes the other coins");
        wait_in(window, &what, 20, |snap| {
            swap_of(snap, &first.id).is_some_and(|swap| matches!(swap.stage, Stage::Claiming | Stage::Ended(_)))
        });
        save(window, &format!("{at}-{side}-side-claiming"));
        let what = format!("the {side} side's swap ends");
        wait_in(window, &what, 20, |snap| swap_of(snap, &first.id).is_some_and(|swap| matches!(swap.stage, Stage::Ended(_))));
        assert_eq!(stage_of(window, &first.id), Some(Stage::Ended(Outcome::Swapped)));
    };
    first_done(&mut n, "new", "12");
    save(&mut n, "13-new-side-done");
    first_done(&mut o, "olc", "14");
    save(&mut o, "15-olc-side-done");
    tell(&format!("  OLC side: {} OLC spendable", o.state().snapshot().node.spendable / COIN));

    wait_in(&mut n, "the second lock", 5, |snap| swap_of(snap, &second.id).is_some_and(|swap| swap.stage == Stage::Locked));
    save(&mut n, "16-new-side-waits-to-take-back");
    wait_in(&mut n, "the way back opens", 40, |snap| {
        swap_of(snap, &second.id).is_some_and(|swap| matches!(swap.stage, Stage::Refunding | Stage::Ended(_)))
    });
    save(&mut n, "17-new-side-taking-back");
    wait_in(&mut n, "the coins come back", 10, |snap| swap_of(snap, &second.id).is_some_and(|swap| matches!(swap.stage, Stage::Ended(_))));
    assert_eq!(stage_of(&n, &second.id), Some(Stage::Ended(Outcome::Refunded)));
    save(&mut n, "18-new-side-took-back");
    tell(&format!("done: first {} second {}", first.id, second.id));
}

/// An offer's code, big enough for a phone camera, from the new coin's folder a swap run left.
#[test]
#[ignore = "needs the folder live_swap_and_a_way_back leaves"]
fn live_offer_code() {
    let Some(peer) = live_peer() else { return };
    let mut n = start(&live_root_kept("N"), NEW_SIDE_PORT, vec![peer], 1_000);
    // The folder opens on OLC with these settings, and the coin made in the run is still known.
    wait_in(&mut n, "OLC runs", 2, |snap| snap.node.coin.as_ref().is_some_and(|coin| coin.ticker == "OLC"));
    wait_in(&mut n, "SWT among the coins to swap for", 1, |snap| snap.swaps.coins.iter().any(|(_, ticker)| ticker == "SWT"));
    let known = ids(&n);
    fill(&mut n, Role::TextInput, "주는 양 (OLC)", "1");
    fill(&mut n, Role::TextInput, "받는 양 (SWT)", "1");
    press_till(&mut n, "제안 만들기", |window| new_swap(window, &known).is_some());
    let offer = new_swap(&n, &known).unwrap();
    tell(&format!("  offer text: {} letters", offer.text.as_deref().map_or(0, str::len)));
    press_till(&mut n, "QR 코드로 보기", |window| has(window, "QR 코드 숨기기"));
    save(&mut n, "04b-new-side-offer-as-qr-now");
    press_till(&mut n, "취소", |window| stage_of(window, &offer.id) != Some(Stage::WaitingForAnswer));
}
