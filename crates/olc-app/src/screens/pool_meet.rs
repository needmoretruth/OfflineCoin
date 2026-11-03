//! Two computers mine together, as Bitcoin's pools work: one starts a pool from its pool page with
//! its own miner as the first worker, the other types the pool's address and a name and joins, and
//! a block the pool finds pays both. Both run here on 127.0.0.1, and what a person sees is saved
//! at 1280 by 800, dark, in basic Korean and in expert English.
use super::live::{live_folder, live_root, live_settings, tell};
use super::live_swap::{fill, has, press, press_till, wait_in};
use super::shots::shot;
use super::{app, harness, port_of_scene, Window};
use crate::core::{BlockState, Link, PoolSettings, Snapshot, Who};
use crate::i18n::{Lang, Mode};
use egui::accesskit::Role;
use egui::{Key, Modifiers};
use egui_kittest::kittest::{By, Queryable};
use olc_core::coin::CoinConfig;
use olc_store::coin_file::{new_coin, NewCoin};
use std::net::{IpAddr, Ipv4Addr};
use std::path::{Path, PathBuf};

const SIZE: (u32, u32) = (1280, 800);

fn coin() -> CoinConfig {
    let now = super::live_swap::unix_ms();
    let values = vec![
        ("name".into(), "Pool Test".into()),
        ("ticker".into(), "PLT".into()),
        ("rules.block_time_ms".into(), "8000".into()),
        ("genesis_time_ms".into(), now.to_string()),
    ];
    let mut coin = new_coin(&NewCoin { values }).unwrap();
    // About what two capped miners find in the block time, so blocks come about as often as the
    // coin says from the first one, and the refused shares are the few a real pool sees.
    coin.initial_difficulty = 8_000;
    coin.rules.min_difficulty = 1;
    coin
}

fn shots() -> PathBuf {
    live_folder().join("pool-shots")
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

/// A window on its own coin, listening on 127.0.0.1 only, with its pool held there too.
fn window(root: &Path, port: u16, peers: &[String], pool: PoolSettings, coin: &CoinConfig) -> Window {
    let mut settings = live_settings(port, peers);
    settings.hash_cap = 1_000;
    settings.mine_on_start = false;
    settings.pool = pool;
    let mut window = harness(app(root.to_path_buf(), settings, None), SIZE);
    window.state().send(crate::core::Command::Join { coin: Box::new(coin.clone()), address: None });
    wait_in(&mut window, "the coin runs", 2, |snap| snap.node.coin.as_ref().is_some_and(|coin| coin.ticker == "PLT"));
    window.state_mut().open("page.mining");
    window.run_steps(3);
    scroll_to(&mut window, "이 컴퓨터에서 풀 열기");
    window
}

fn scroll_to(window: &mut Window, label: &str) {
    if let Some(node) = window.query_all(By::new().label(label)).next() {
        node.scroll_to_me();
    }
    window.run_steps(3);
}

/// Replaces what a field holds, as a person selects it all and types.
fn retype(window: &mut Window, label: &str, text: &str) {
    window.query(By::new().role(Role::TextInput).label(label)).unwrap_or_else(|| panic!("no field {label}")).focus();
    window.run_steps(2);
    window.key_press_modifiers(Modifiers::COMMAND, Key::A);
    window.run_steps(1);
    window.query(By::new().role(Role::TextInput).label(label)).unwrap_or_else(|| panic!("no field {label}")).type_text(text);
    window.run_steps(3);
}

fn report(window: &Window, who: &str) {
    let snap = window.state().snapshot();
    let pool = &snap.pool;
    let workers: Vec<String> = pool.workers.iter().map(|row| format!("{} {}/{} paid {}", row.worker, row.accepted, row.refused, row.paid)).collect();
    let links: Vec<String> = pool
        .links
        .iter()
        .map(|row| format!("{:?} {:?} {}/{} kept back {} {:?}", row.join.who, row.view.link, row.view.accepted, row.view.refused, row.view.stale, row.view.last_refusal))
        .collect();
    tell(&format!("  {who}: running {}, workers {workers:?}, links {links:?}, found {}", pool.running, pool.found.len()));
}

/// The addresses the kernel says listen on `port`, from its own table: what a test opened.
fn listening_on(port: u16) -> Vec<Ipv4Addr> {
    const LISTEN: &str = "0A";
    let table = std::fs::read_to_string("/proc/net/tcp").expect("the kernel's table");
    let mut found = Vec::new();
    for line in table.lines().skip(1) {
        let fields: Vec<&str> = line.split_whitespace().collect();
        let Some((address, at)) = fields.get(1).and_then(|local| local.split_once(':')) else {
            continue;
        };
        if fields.get(3) != Some(&LISTEN) || u16::from_str_radix(at, 16).ok() != Some(port) {
            continue;
        }
        if let Ok(word) = u32::from_str_radix(address, 16) {
            found.push(Ipv4Addr::from(word.swap_bytes()));
        }
    }
    let six = std::fs::read_to_string("/proc/net/tcp6").unwrap_or_default();
    let hex_port = format!(":{port:04X} ");
    assert!(!six.lines().any(|line| line.contains(&hex_port) && line.contains(" 0A ")), "nothing listens on IPv6");
    found
}

fn shared_out(snap: &Snapshot) -> bool {
    // The newest block shared out is the one the page opens, so it is the one that shows both.
    snap.pool.found.iter().find(|row| row.state == BlockState::Paid).is_some_and(|row| row.parts.len() >= 2)
}

#[test]
#[ignore = "mines a coin for minutes: run it by name"]
fn a_second_computer_joins_the_pool_by_address_and_both_are_paid() {
    let (a_root, b_root) = (live_root("POOL-A"), live_root("POOL-B"));
    let coin = coin();
    let a_port = port_of_scene(46);
    let pool_port = port_of_scene(48);
    // The pool's computer lets others join, as the picture shows; on this one test computer it
    // still listens on 127.0.0.1 alone and shows a stand-in network address, looked up nowhere.
    let stand_in = Some(IpAddr::V4(Ipv4Addr::new(192, 168, 0, 5)));
    let pool = PoolSettings { port: pool_port, open: true, stand_in, ..PoolSettings::default() };
    let mut a = window(&a_root, a_port, &[], pool, &coin);
    let mut b = window(&b_root, port_of_scene(47), &[format!("127.0.0.1:{a_port}")], PoolSettings::default(), &coin);

    // A: the app's miner is the first worker, and every miner it knows is listed.
    save(&mut a, "pool-00-form-ko-basic");
    assert!(a.query_all(By::new().label_contains("이 컴퓨터")).next().is_some(), "this computer is not listed");
    press_till(&mut a, "풀 시작", |window| window.state().snapshot().pool.running);
    report(&a, "a after start");
    let bound = listening_on(pool_port);
    tell(&format!("  the pool listens on {bound:?}"));
    assert_eq!(bound, vec![Ipv4Addr::LOCALHOST], "the pool's port is open to this computer alone");
    wait_in(&mut a, "the stand-in address", 1, |snap| snap.pool.reach.first().is_some_and(|at| at.starts_with("192.168.0.5:")));

    // B types the pool's address and its own name, and joins.
    press(&mut b, "풀에 들어가기");
    fill(&mut b, Role::TextInput, "192.168.0.5:3333", &format!("127.0.0.1:{pool_port}"));
    let user = format!("{}.laptop", b.state().snapshot().node.miner);
    retype(&mut b, "이름", &user);
    save(&mut b, "pool-03-join-form-ko-basic");
    press(&mut b, "들어가기");
    let working = |snap: &Snapshot| snap.pool.link_of(Who::Own).is_some_and(|link| link.view.link == Link::Working && link.view.accepted > 0);
    wait_in(&mut b, "b's shares are taken", 3, working);
    report(&b, "b joined");
    save(&mut b, "pool-03-joined-ko-basic");
    speak(&mut b, Mode::Expert, Lang::En);
    save(&mut b, "pool-03-joined-en-expert");
    speak(&mut b, Mode::Basic, Lang::Ko);

    // A: both computers work in the pool.
    let both = |snap: &Snapshot| snap.pool.workers.iter().filter(|row| row.accepted > 0).count() >= 2;
    wait_in(&mut a, "both work", 3, both);
    report(&a, "a with both");
    save(&mut a, "pool-01-running-ko-basic");
    speak(&mut a, Mode::Expert, Lang::En);
    save(&mut a, "pool-01-running-en-expert");
    speak(&mut a, Mode::Basic, Lang::Ko);

    // A block the pool found matures and is shared out to both.
    wait_in(&mut a, "a block shared out", 10, shared_out);
    report(&a, "a paid");
    scroll_to(&mut a, "풀이 찾은 블록");
    save(&mut a, "pool-02-payout-ko-basic");
    speak(&mut a, Mode::Expert, Lang::En);
    scroll_to(&mut a, "Blocks the pool found");
    save(&mut a, "pool-02-payout-en-expert");
    speak(&mut a, Mode::Basic, Lang::Ko);
    let snap = a.state().snapshot();
    // A round only one computer worked in pays it alone; over the blocks shared out, both are paid.
    let paid: Vec<_> = snap.pool.found.iter().filter(|row| row.state == BlockState::Paid).collect();
    let laptop = paid.iter().flat_map(|row| &row.parts).filter(|(name, _)| name.ends_with(".laptop"));
    assert!(laptop.map(|(_, amount)| *amount).sum::<u64>() > 0, "the other computer is paid: {paid:?}");
    // The pool's own miner sends what it found before the pool takes a new block, so few of its
    // shares are refused as old work: 5 of 21 were before it did.
    let own = snap.pool.workers.iter().find(|row| row.here == Some(Who::Own)).expect("this computer works");
    assert!(own.refused * 10 <= own.accepted, "{} refused of {}", own.refused, own.accepted);
    // Two shares that both seal the same parent make one block, not two of one height.
    let mut heights: Vec<u64> = snap.pool.found.iter().map(|row| row.height).collect();
    heights.dedup();
    assert_eq!(heights.len(), snap.pool.found.len(), "one block a height: {heights:?}");
    assert!(has(&a, "풀 멈추기"));
    tell("done: a second computer joined the pool by address and both were paid");
}
