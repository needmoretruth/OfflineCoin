//! Two computers that have never met open a channel from one screen, as Lightning does: one shows
//! its channel address, the other pastes it into one field, and the channel opens and carries a
//! payment. Both run here on 127.0.0.1, and every step a person sees is saved at 1280 by 800, dark,
//! in basic Korean and in expert English.
use super::live::{live_folder, live_root, live_settings, tell};
use super::live_swap::{fill, has, press, press_till, wait_in};
use super::shots::shot;
use super::{app, harness, port_of_scene, Window};
use crate::core::host_channels::Phase;
use crate::core::{ChannelOrder, Command};
use crate::i18n::{Lang, Mode};
use egui::accesskit::Role;
use egui_kittest::kittest::{By, Queryable};
use olc_core::coin::{CoinConfig, COIN};
use olc_store::coin_file::{new_coin, NewCoin};
use std::path::{Path, PathBuf};

const SIZE: (u32, u32) = (1280, 800);

fn coin() -> CoinConfig {
    let now = super::live_swap::unix_ms();
    let values = vec![
        ("name".into(), "Channel Test".into()),
        ("ticker".into(), "CHT".into()),
        ("rules.block_time_ms".into(), "4000".into()),
        ("genesis_time_ms".into(), now.to_string()),
    ];
    new_coin(&NewCoin { values }).unwrap()
}

fn shots() -> PathBuf {
    live_folder().join("channel-shots")
}

fn save(window: &mut Window, name: &str) {
    shot(window, &shots(), name);
    tell(&format!("  shot {name}"));
}

fn speak(window: &mut Window, mode: Mode, lang: Lang) {
    let settings = window.state_mut().settings_mut();
    settings.mode = mode;
    settings.language = lang;
    window.run_steps(3);
}

/// A window on its own coin, listening on 127.0.0.1 only and dialling nobody.
fn window(root: &Path, port: u16, mines: bool, coin: &CoinConfig) -> Window {
    let mut settings = live_settings(port, &[]);
    settings.hash_cap = 1_000;
    settings.mine_on_start = mines;
    let mut window = harness(app(root.to_path_buf(), settings, None), SIZE);
    window.state().send(Command::Join { coin: Box::new(coin.clone()), address: None });
    wait_in(&mut window, "the coin runs", 2, |snap| snap.node.coin.as_ref().is_some_and(|coin| coin.ticker == "CHT"));
    window.state_mut().open("page.channels");
    window
}

/// What the channel side of a window says now, for the run's log.
fn report(window: &Window, who: &str) {
    let snap = window.state().snapshot();
    let channels = &snap.node.channels;
    let notice = snap.notice.as_ref().map(|notice| format!("{} {}", notice.key, notice.detail));
    let events: Vec<String> = channels.events.iter().take(3).map(|event| format!("{} {}", event.key, event.detail)).collect();
    tell(&format!(
        "  {who}: notice {notice:?}, reaching {:?}, events {events:?}, connections {}, heard {}",
        channels.reaching,
        snap.node.connections.rows.len(),
        channels.peers.len()
    ));
}

fn scroll_to(window: &mut Window, label: &str) {
    if let Some(node) = window.query_all(By::new().label(label)).next() {
        node.scroll_to_me();
    }
    window.run_steps(3);
}

/// One computer: its channel address and the one field are on the page from the start.
#[test]
fn the_channels_page_shows_my_address_and_takes_the_other_one() {
    let root = live_root("CH-one");
    let mut one = window(&root, port_of_scene(45), false, &coin());
    wait_in(&mut one, "the channel address", 2, |snap| !snap.node.channels.address.is_empty());
    let address = one.state().snapshot().node.channels.address.clone();
    assert!(address.contains("@127.0.0.1:"), "{address}");
    assert!(has(&one, "복사"), "no copy button");
    assert!(has(&one, "QR 보기"), "no code button");
    assert!(one.query(By::new().role(Role::TextInput).label("상대 채널 주소 붙여 넣기")).is_some());
    // This computer listens on 127.0.0.1 only, and the page says how to change that.
    assert!(one.query(By::new().label_contains("이 컴퓨터 안에서만")).is_some());
    assert!(has(&one, "켜기"), "no switch beside the note");
    // The address stays on one line: the key's middle is left out, the place to dial is whole.
    let (key, at) = address.split_once('@').unwrap();
    let shown = format!("{}...{}@{at}", &key[..6], &key[key.len() - 4..]);
    assert!(one.query(By::new().label(shown.as_str())).is_some(), "{shown} not shown");
    // With no channel, the balance card does not lead the page.
    assert!(one.query(By::new().label("채널에 있는 내 코인")).is_none());
    // A pasted key of the wrong length is said at once.
    fill(&mut one, Role::TextInput, "상대 채널 주소 붙여 넣기", "02abcd@127.0.0.1:7000");
    assert!(one.query(By::new().label_contains("온전한 채널 주소가 아닙니다")).is_some());
}

#[test]
#[ignore = "mines a coin for minutes: run it by name"]
fn two_computers_that_never_met_open_a_channel_by_address_and_pay() {
    let (a_root, b_root) = (live_root("CH-A"), live_root("CH-B"));
    // Both follow the same coin, as two people who joined it would.
    let coin = coin();
    let mut a = window(&a_root, port_of_scene(41), true, &coin);
    let mut b = window(&b_root, port_of_scene(42), false, &coin);

    // B has no coins and no channels yet: its page shows the address to hand over.
    wait_in(&mut b, "b's channel address", 2, |snap| !snap.node.channels.address.is_empty());
    save(&mut b, "ch-01-empty-with-address-ko-basic");
    speak(&mut b, Mode::Expert, Lang::En);
    save(&mut b, "ch-01-empty-with-address-en-expert");
    speak(&mut b, Mode::Basic, Lang::Ko);
    press_till(&mut b, "QR 보기", |window| has(window, "QR 숨기기"));
    save(&mut b, "ch-02-address-code-ko-basic");
    press_till(&mut b, "QR 숨기기", |window| has(window, "QR 보기"));

    wait_in(&mut a, "coins to open with", 10, |snap| snap.node.spendable >= 20 * COIN);
    let address = b.state().snapshot().node.channels.address.clone();
    tell(&format!("  b's address: {address}"));
    fill(&mut a, Role::TextInput, "상대 채널 주소 붙여 넣기", &address);
    fill(&mut a, Role::TextInput, "넣을 금액", "10");
    save(&mut a, "ch-03-pasted-address-ko-basic");
    press(&mut a, "열기");
    a.query_all_by_label("열기").last().expect("the slip's own button").click();
    a.run_steps(3);
    report(&a, "a after the press");
    let opened = |phase: Phase| move |snap: &crate::core::Snapshot| snap.node.channels.channels.iter().any(|channel| channel.phase == phase);
    wait_in(&mut a, "the channel is asked for", 2, |snap| !snap.node.channels.channels.is_empty());
    save(&mut a, "ch-04-opening-ko-basic");
    wait_in(&mut a, "the channel opens", 10, opened(Phase::Open));
    wait_in(&mut b, "b sees it open", 5, opened(Phase::Open));

    // B asks for 3 and A pays it through the channel.
    let requests = |count: usize| move |window: &Window| window.state().snapshot().node.channels.invoices.len() >= count;
    scroll_to(&mut b, "요청 만들기");
    fill(&mut b, Role::TextInput, "금액", "3");
    press_till(&mut b, "요청 만들기", requests(1));
    let request = b.state().snapshot().node.channels.invoices[0].text.clone();
    a.state().send(Command::Channel(ChannelOrder::Pay { text: request }));
    wait_in(&mut b, "the payment arrives", 3, |snap| snap.node.channels.invoices.iter().any(|invoice| invoice.paid));
    wait_in(&mut a, "a's side moved", 1, |snap| {
        snap.node.channels.channels.iter().any(|channel| channel.phase == Phase::Open && channel.theirs >= 3 * COIN)
    });
    // The notice that it is on its way gives way to the one saying it arrived.
    wait_in(&mut a, "the sending notice ends", 1, |snap| snap.notice.as_ref().is_some_and(|notice| notice.key == "ch.said_paid"));
    // A second request stays waiting, so the list shows both states.
    scroll_to(&mut b, "요청 만들기");
    fill(&mut b, Role::TextInput, "금액", "1");
    press_till(&mut b, "요청 만들기", requests(2));

    scroll_to(&mut a, "자세히");
    press(&mut a, "자세히");
    save(&mut a, "ch-05-open-after-payment-ko-basic");
    speak(&mut a, Mode::Expert, Lang::En);
    save(&mut a, "ch-05-open-after-payment-en-expert");
    speak(&mut a, Mode::Basic, Lang::Ko);
    scroll_to(&mut b, "내 요청");
    save(&mut b, "ch-06-request-list-ko-basic");
    speak(&mut b, Mode::Expert, Lang::En);
    scroll_to(&mut b, "Your requests");
    save(&mut b, "ch-06-request-list-en-expert");

    a.state_mut().open("page.coins");
    // The other computer goes by the name its hello carried, not by where it connects from.
    wait_in(&mut a, "b's name on its connection", 1, |snap| snap.node.connections.rows.iter().any(|row| !row.name.is_empty()));
    a.run_steps(3);
    scroll_to(&mut a, "연결된 컴퓨터");
    save(&mut a, "ch-07-peers-ko-basic");
    speak(&mut a, Mode::Expert, Lang::En);
    scroll_to(&mut a, "Peers");
    save(&mut a, "ch-07-peers-en-expert");
    tell("done: a channel opened by address and carried a payment");
}
