//! A release check on a copy of a person's own data folder. The app opens it as it is, with every
//! setting and miner the person saved in an older version, and the speeds on this computer stay
//! inside the person's limit, follow the thread cap, and the network figures read. New data alone
//! would miss what an older version left behind. Run it by hand before a release, on a copy:
//!
//! `OLC_REAL_DATA=<copy> OLC_SHOTS=<folder> cargo test -p olc-app --lib real_data -- --ignored --nocapture`
//!
//! The copy must not be a folder an open app uses: its port is changed and USB, the local network
//! and saved peers are left off, so it touches nothing outside itself.
use super::shots::shot;
use super::{app, harness, node_program, port_of_scene, wait_until, Window, SETTLE_STEPS};
use crate::core::{Command, Snapshot};
use crate::i18n::Lang;
use crate::settings::Settings;
use egui::accesskit::Role;
use egui::{Key, Modifiers};
use egui_kittest::kittest::{By, Queryable};
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Every miner's measured speed on this computer together, as the nodes last said it.
fn together(snap: &Snapshot) -> u64 {
    let own = if snap.node.mining { snap.node.hashrate } else { 0 };
    let here = snap.local.iter().filter_map(|view| view.status.as_ref()).filter(|status| status.mining);
    own + here.map(|status| status.hashrate).sum::<u64>()
}

/// The highest and the mean of that sum, read twice a second for `seconds`.
fn watch(window: &mut Window, seconds: u64) -> (u64, u64) {
    let started = Instant::now();
    let mut readings = Vec::new();
    while started.elapsed() < Duration::from_secs(seconds) {
        window.run_steps(1);
        readings.push(together(&window.state().snapshot()));
        std::thread::sleep(Duration::from_millis(500));
    }
    let most = readings.iter().copied().max().unwrap_or(0);
    (most, readings.iter().sum::<u64>() / readings.len().max(1) as u64)
}

/// Replaces what a field holds and presses Enter, as a person selects it all and types.
fn retype(window: &mut Window, label: &str, text: &str) {
    let field = window.query(By::new().role(Role::TextInput).label(label)).unwrap_or_else(|| panic!("no field {label}"));
    field.focus();
    window.run_steps(2);
    window.key_press_modifiers(Modifiers::COMMAND, Key::A);
    window.run_steps(1);
    let field = window.query(By::new().role(Role::TextInput).label(label)).unwrap_or_else(|| panic!("no field {label}"));
    field.type_text(text);
    window.run_steps(2);
    window.key_press(Key::Enter);
    window.run_steps(3);
}

fn save(window: &mut Window, name: &str) {
    if let Some(folder) = std::env::var_os("OLC_SHOTS").map(PathBuf::from) {
        shot(window, &folder, name);
    }
}

/// The highest speed after the thread cap moved to `threads`, once it settled.
fn highest_on(window: &mut Window, threads: usize) -> u64 {
    window.state_mut().open("page.settings");
    window.run_steps(SETTLE_STEPS);
    let before = window.state().snapshot().full_rate;
    retype(window, "Mining threads", &threads.to_string());
    let started = Instant::now();
    while window.state().snapshot().full_rate == before && started.elapsed() < Duration::from_secs(20) {
        window.run_steps(1);
        std::thread::sleep(Duration::from_millis(100));
    }
    window.state().snapshot().full_rate
}

#[test]
#[ignore = "needs OLC_REAL_DATA, a copy of a person's data folder"]
fn a_persons_own_data_opens_and_mines_inside_their_limit() {
    let Some(root) = std::env::var_os("OLC_REAL_DATA").map(PathBuf::from) else {
        return;
    };
    let mut settings = Settings::load(&root);
    settings.port = port_of_scene(70);
    settings.scan_lan = false;
    settings.broadcast = false;
    settings.listen_lan = false;
    settings.auto_update = false;
    settings.auto_boards.clear();
    settings.peers.clear();
    // The field names below are the English ones; the pictures are taken in the person's language.
    let language = settings.language;
    settings.language = Lang::En;
    let limit = settings.hash_cap;
    let saved: Vec<u32> = settings.nodes.iter().map(|spec| spec.id).collect();
    let opened = Instant::now();
    let mut window = harness(app(root, settings, Some(node_program())), (1280, 800));
    wait_until(&mut window, |app| app.snapshot().full_rate > 0 && app.snapshot().node.height > 0);
    let snap = window.state().snapshot();
    println!(
        "opened in {:.1} s at height {}: highest {} on the saved threads, limit {limit}, budget {:?}",
        opened.elapsed().as_secs_f64(),
        snap.node.height,
        snap.full_rate,
        snap.budget
    );

    // Mining as the person does, with every saved miner switched on.
    window.state().send(Command::SetMining(true));
    for id in &saved {
        window.state().send(Command::StartNode(*id));
        window.state().send(Command::NodeMining { id: *id, on: true });
    }
    let started = Instant::now();
    while together(&window.state().snapshot()) == 0 && started.elapsed() < Duration::from_secs(60) {
        window.run_steps(1);
        std::thread::sleep(Duration::from_millis(200));
    }
    let (most, mean) = watch(&mut window, 60);
    let snap = window.state().snapshot();
    let ceiling = if limit > 0 { limit.min(snap.full_rate) } else { snap.full_rate };
    let parts: Vec<_> = snap.local.iter().map(|view| (view.spec.name.clone(), view.status.as_ref().map(|status| (status.mining, status.hashrate, status.cap)))).collect();
    println!("mining a minute: highest {most}, mean {mean} against {ceiling}; this app {} of cap {}; miners {parts:?}", snap.node.hashrate, snap.node.cap);
    assert!(mean as f64 <= ceiling as f64 * 1.03, "the miners mean {mean}, over {ceiling}");
    // Every speed is a five second average under its own cap, so no reading stands above the
    // ceiling while the miners keep to it.
    assert!(most <= ceiling, "the miners read {most}, over {ceiling}");

    let threads = crate::settings::cores();
    let wide = highest_on(&mut window, threads);
    let one = highest_on(&mut window, 1);
    println!("highest speed: {wide} on {threads} threads, {one} on 1");
    assert!(wide > one, "the thread cap moves the highest speed: {wide} on {threads}, {one} on 1");

    let chain = &window.state().snapshot().node.chain;
    println!(
        "network: this computer {}, all {}, average {:?}, share {:?}, read from {:?}",
        chain.hashrate_here,
        chain.hashrate_all,
        chain.average,
        chain.share(),
        chain.network
    );
    window.state_mut().settings_mut().language = language;
    for page in ["page.home", "page.mining"] {
        window.state_mut().open(page);
        window.run_steps(SETTLE_STEPS * 3);
        save(&mut window, &format!("real-data-{}", page.trim_start_matches("page.")));
    }
    window.state().send(Command::SetMining(false));
    for id in &saved {
        window.state().send(Command::StopNode(*id));
    }
    window.run_steps(SETTLE_STEPS);
}
