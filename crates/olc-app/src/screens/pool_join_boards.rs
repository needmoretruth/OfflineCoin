//! A friend opens a pool on another computer. From the pool page, the owner picks this computer
//! and the boards and joins all of them at once: board one on the cable, board zero heard only by
//! radio through board one, and board two once its old build is updated. The friend's pool takes
//! each one's shares under its own name. Two real nodes and the friend's pool run on 127.0.0.1;
//! the boards are the stand-ins of `fake_boards`, since no test may touch a board.
//! The pictures go to `.scratch/live/pool-join-shots`, at 1280 by 800, dark.
use super::fake_boards::{Boards, ONE, TWO, ZERO};
use super::live::{live_folder, live_root, live_settings, tell};
use super::live_swap::{fill, press, wait_in};
use super::shots::shot;
use super::{harness, port_of_scene, Window};
use crate::app::App;
use crate::core::{Command, Core, CoreLaunch, PoolOrder, PoolSettings, Snapshot, Who};
use crate::i18n::{Lang, Mode};
use crate::settings::Paths;
use egui::accesskit::{Role, Toggled};
use egui_kittest::kittest::{By, NodeT, Queryable};
use olc_core::coin::CoinConfig;
use olc_service::fleet::FleetPort;
use olc_store::coin_file::{new_coin, NewCoin};
use std::path::Path;
use std::sync::Arc;

const SIZE: (u32, u32) = (1280, 800);
/// A share every few seconds from each board's few hundred thousand nonces a second.
const SHARE_DIFFICULTY: u64 = 300;

fn coin() -> CoinConfig {
    let now = super::live_swap::unix_ms();
    let values = vec![
        ("name".into(), "Join Test".into()),
        ("ticker".into(), "JNT".into()),
        ("rules.block_time_ms".into(), "8000".into()),
        ("genesis_time_ms".into(), now.to_string()),
    ];
    let mut coin = new_coin(&NewCoin { values }).unwrap();
    coin.initial_difficulty = 200_000;
    coin.rules.min_difficulty = 1;
    coin
}

fn save(window: &mut Window, name: &str) {
    shot(window, &live_folder().join("pool-join-shots"), name);
    tell(&format!("  shot {name}"));
}

fn speak(window: &mut Window, mode: Mode, lang: Lang) {
    let settings = window.state_mut().settings_mut();
    settings.mode = mode;
    settings.language = lang;
    window.run_steps(3);
}

/// A window on the test coin, listening on 127.0.0.1 only, with `fleet` as its boards.
fn window(root: &Path, port: u16, peers: &[String], fleet: Option<Arc<dyn FleetPort>>, coin: &CoinConfig) -> Window {
    let mut settings = live_settings(port, peers);
    settings.hash_cap = 1_000;
    settings.mine_on_start = false;
    let launch = CoreLaunch { paths: Paths { root: root.to_path_buf() }, fleet, usb: false, node_program: None };
    let core = Core::start(launch, settings.clone()).unwrap();
    let mut app = App::new(core, settings, None);
    app.set_folder(root.to_path_buf());
    let mut window = harness(app, SIZE);
    window.state().send(Command::Join { coin: Box::new(coin.clone()), address: None });
    wait_in(&mut window, "the coin runs", 2, |snap| snap.node.coin.as_ref().is_some_and(|coin| coin.ticker == "JNT"));
    window.state_mut().open("page.mining");
    window.run_steps(3);
    window
}

fn scroll_to(window: &mut Window, label: &str) {
    if let Some(node) = window.query_all(By::new().label(label)).next() {
        node.scroll_to_me();
    }
    window.run_steps(3);
}

fn top_of<'a>(node: &egui_kittest::Node<'a>) -> f64 {
    node.accesskit_node().bounding_box().map_or(f64::MAX, |rect| rect.y0)
}

/// The switch of the join form: the first of its name under "Who joins". The own pool's list of
/// the same names sits above or below the form, as the page orders the two cards.
fn switch_on(window: &mut Window, label: &str) {
    let heading = window.query(By::new().label("함께 들어갈 기기")).expect("the join form's list");
    let below = top_of(&heading);
    let mut found: Vec<_> = window.query_all(By::new().role(Role::Switch).label(label)).filter(|node| top_of(node) > below).collect();
    found.sort_by(|a, b| top_of(a).total_cmp(&top_of(b)));
    let switch = found.into_iter().next().unwrap_or_else(|| panic!("no switch {label} in the join form"));
    if !matches!(switch.accesskit_node().toggled(), Some(Toggled::True)) {
        switch.click();
    }
    window.run_steps(3);
}

fn shown(window: &Window, text: &str) -> bool {
    window.query_all(By::new().label_contains(text)).next().is_some()
}

fn accepted(snap: &Snapshot, who: Who) -> u64 {
    snap.pool.link_of(who).map_or(0, |link| link.view.accepted)
}

/// What the friend's pool took from the worker whose name ends in `name`.
fn taken_by_friend(snap: &Snapshot, name: &str) -> u64 {
    let ending = format!(".{name}");
    snap.pool.workers.iter().filter(|row| row.name.ends_with(&ending)).map(|row| row.accepted).sum()
}

fn report(window: &Window, who: &str) {
    let snap = window.state().snapshot();
    let workers: Vec<String> = snap.pool.workers.iter().map(|row| format!("{} {}/{}", row.worker, row.accepted, row.refused)).collect();
    let links: Vec<String> = snap.pool.links.iter().map(|row| format!("{:?} {:?} {}/{}", row.join.who, row.view.link, row.view.accepted, row.view.refused)).collect();
    tell(&format!("  {who}: workers {workers:?}, links {links:?}"));
}

/// Opens the join form and types the friend's pool address.
fn open_form(window: &mut Window, pool: &str) {
    scroll_to(window, "다른 컴퓨터의 풀에 들어가기");
    // The join card is the last on the page; a miner's own row may offer joining too.
    let opens = window.query_all(By::new().role(Role::Button).label("풀에 들어가기")).count();
    tell(&format!("  {opens} join buttons on the page"));
    window.query_all(By::new().role(Role::Button).label("풀에 들어가기")).last().expect("the join button").click();
    window.run_steps(3);
    fill(window, Role::TextInput, "192.168.0.5:3333", pool);
    scroll_to(window, "함께 들어갈 기기");
}

#[test]
#[ignore = "runs two nodes and a pool for a minute or two: run it by name"]
fn this_computer_and_the_boards_join_a_pool_on_another_computer() {
    let coin = coin();
    let (friend_port, pool_port) = (port_of_scene(52), port_of_scene(54));
    let mut friend = window(&live_root("JOIN-FRIEND"), friend_port, &[], None, &coin);
    let pool = PoolSettings { port: pool_port, fixed_difficulty: SHARE_DIFFICULTY, workers: Vec::new(), ..PoolSettings::default() };
    friend.state().send(Command::Pool(PoolOrder::Start(pool)));
    wait_in(&mut friend, "the friend's pool runs", 1, |snap| snap.pool.running);

    let boards = Boards::new(&coin, false);
    let fleet: Arc<dyn FleetPort> = boards.clone();
    let peers = [format!("127.0.0.1:{friend_port}")];
    let mut me = window(&live_root("JOIN-ME"), port_of_scene(53), &peers, Some(fleet), &coin);
    let heard = |snap: &Snapshot| snap.pool.choices.iter().any(|choice| choice.who == Some(Who::Lent(ZERO)));
    wait_in(&mut me, "board zero heard through board one", 1, heard);

    // The form lists this computer and every board, board zero through board one, and board two
    // with the reason its build cannot take pool work.
    let address = format!("127.0.0.1:{pool_port}");
    open_form(&mut me, &address);
    for name in ["이 컴퓨터", "보드원", "보드투", "보드제로"] {
        assert!(me.query_all(By::new().role(Role::Switch).label(name)).count() >= 1, "{name} is not in the form");
    }
    assert!(shown(&me, "보드원 통해 연결"), "board zero does not say it comes through board one");
    assert!(shown(&me, "보드 프로그램이 오래돼서 풀 일을 받지 못합니다"), "board two does not say why it cannot join");
    switch_on(&mut me, "보드원");
    switch_on(&mut me, "보드제로");
    // The form down to its button, as a person sees it before pressing.
    scroll_to(&mut me, "들어가기");
    save(&mut me, "join-01-form-ko-basic");
    speak(&mut me, Mode::Expert, Lang::En);
    scroll_to(&mut me, "Join");
    save(&mut me, "join-01-form-en-expert");
    speak(&mut me, Mode::Basic, Lang::Ko);
    scroll_to(&mut me, "함께 들어갈 기기");
    press(&mut me, "들어가기");

    // Each one works in the friend's pool under its own name, and the friend's pool takes its shares.
    let joined = [Who::Own, Who::Board(ONE), Who::Lent(ZERO)];
    let working = |snap: &Snapshot| joined.iter().all(|who| accepted(snap, *who) > 0);
    wait_in(&mut me, "this computer and both boards are taken", 3, working);
    report(&me, "me");
    let all_taken = |snap: &Snapshot| ["보드원", "보드제로"].iter().all(|name| taken_by_friend(snap, name) > 0);
    wait_in(&mut friend, "the friend's pool takes the boards' shares", 1, all_taken);
    report(&friend, "friend");
    scroll_to(&mut me, "다른 컴퓨터의 풀에 들어가기");
    save(&mut me, "join-02-joined-ko-basic");
    speak(&mut me, Mode::Expert, Lang::En);
    scroll_to(&mut me, "Join a pool on another computer");
    save(&mut me, "join-02-joined-en-expert");
    speak(&mut me, Mode::Basic, Lang::Ko);

    // Board two, once updated, joins too; those already in the pool are not offered again.
    boards.update_two();
    let offered = |snap: &Snapshot| snap.pool.choices.iter().any(|choice| choice.who == Some(Who::Board(TWO)) && choice.why_not.is_none());
    wait_in(&mut me, "board two is updated", 1, offered);
    open_form(&mut me, &address);
    let heading = top_of(&me.query(By::new().label("함께 들어갈 기기")).expect("the join form's list"));
    let join = top_of(&me.query(By::new().role(Role::Button).label("들어가기")).expect("the join button"));
    let in_form = |name: &str| me.query_all(By::new().role(Role::Switch).label(name)).filter(|node| (heading..join).contains(&top_of(node))).count();
    assert_eq!((in_form("보드제로"), in_form("보드원"), in_form("보드투")), (0, 0, 1), "only board two is offered again");
    switch_on(&mut me, "보드투");
    save(&mut me, "join-03-board-two-ko-basic");
    press(&mut me, "들어가기");
    me.run_steps(5);
    report(&me, "me after board two joined");
    tell(&format!("  error {:?}", me.state().snapshot().pool.error));
    wait_in(&mut friend, "the friend's pool takes board two's shares", 2, |snap| taken_by_friend(snap, "보드투") > 0);
    report(&friend, "friend with board two");

    // A board leaves, and the others go on.
    scroll_to(&mut me, "다른 컴퓨터의 풀에 들어가기");
    save(&mut me, "join-03-all-four-ko-basic");
    let leaves = me.query_all(By::new().role(Role::Button).label("풀에서 나가기")).count();
    assert!(leaves >= 4, "each joined one has its own leave: {leaves}");
    me.query_all(By::new().role(Role::Button).label("풀에서 나가기")).last().unwrap().click();
    wait_in(&mut me, "one left", 1, |snap| snap.pool.links.len() == 3);
    report(&me, "me after one left");
    boards.stop();
    tell("done: this computer and three boards joined a pool on another computer");
}
