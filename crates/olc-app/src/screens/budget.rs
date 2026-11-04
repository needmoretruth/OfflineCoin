//! One limit for everything OLC Core mines with on this computer, and the network seen as Bitcoin
//! users see it. Every node in these scenes is a real process on 127.0.0.1, and every speed is the
//! one the nodes measured themselves. OLC_SHOTS=<folder> saves what a person sees at 1280 by 800.
use super::live_swap::{press, wait_in};
use super::shots::shot;
use super::{app, harness, node_program, port_of_scene, quiet_settings, scratch, wait_until, Window, SETTLE_STEPS, SIZES};
use crate::app::App;
use crate::core::{Command, PoolOrder, PoolSettings, Snapshot, Who};
use crate::i18n::{Lang, Mode, Voice};
use crate::local::{NodeSpec, Preset, WalletChoice};
use crate::settings::{cores, Theme};
use crate::ui::format;
use egui::accesskit::Role;
use egui::{Key, Modifiers};
use egui_kittest::kittest::{By, NodeT, Queryable};
use std::time::{Duration, Instant};

/// The limit the person sets on Home.
const LIMIT: u64 = 2_000;
/// A node reads its speed once a second, and the cap hands out a second's hashes in hundredths,
/// so one reading may run a little over while the mean holds.
const READING_SLACK: f64 = 1.10;
const MEAN_SLACK: f64 = 1.03;

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

fn speak(window: &mut Window, mode: Mode, lang: Lang) {
    let settings = window.state_mut().settings_mut();
    settings.mode = mode;
    settings.language = lang;
    window.run_steps(SETTLE_STEPS);
}

fn save(window: &mut Window, name: &str) {
    if let Some(folder) = std::env::var_os("OLC_SHOTS").map(std::path::PathBuf::from) {
        shot(window, &folder, name);
    }
}

/// Replaces what a field holds and presses Enter, as a person selects it all and types.
fn retype(window: &mut Window, label: &str, text: &str) {
    let field = || window.query(By::new().role(Role::TextInput).label(label)).unwrap_or_else(|| panic!("no field {label}"));
    field().focus();
    window.run_steps(2);
    window.key_press_modifiers(Modifiers::COMMAND, Key::A);
    window.run_steps(1);
    let field = window.query(By::new().role(Role::TextInput).label(label)).unwrap_or_else(|| panic!("no field {label}"));
    field.type_text(text);
    window.run_steps(2);
    window.key_press(Key::Enter);
    window.run_steps(3);
}

/// Every text on the window that holds `part`.
/// Every text on the screen holding `part`: a line of words keeps its text as its value, a control
/// as its name.
fn texts_with(window: &Window, part: &str) -> Vec<String> {
    let text = |node: egui_kittest::Node<'_>| {
        let node = node.accesskit_node();
        if node.role() == Role::Label { node.value() } else { node.label() }
    };
    window.query_all(By::new().label_contains(part).include_labels()).filter_map(text).collect()
}

fn voice(window: &Window) -> Voice {
    let settings = window.state().settings();
    Voice { lang: settings.language, mode: settings.mode }
}

fn mining_miners(snap: &Snapshot) -> usize {
    snap.local.iter().filter(|view| view.status.as_ref().is_some_and(|status| status.mining)).count()
}

fn wait_for_miners(window: &mut Window, count: usize) {
    wait_until(window, |app: &App| mining_miners(&app.snapshot()) >= count);
}

/// The person sets 2,000 on Home and adds a miner, in basic Korean and again in expert English:
/// the miners start inside the limit and share it, and the window says what a new one starts at
/// before it starts. With no limit a new miner still starts at a share, not the whole computer.
#[test]
fn a_new_miner_shares_the_limit_set_on_home() {
    let mut settings = quiet_settings();
    settings.mine_on_start = true;
    settings.mode = Mode::Expert;
    settings.language = Lang::En;
    settings.theme = Theme::Dark;
    let mut window = harness(app(scratch("budget-share"), settings, Some(node_program())), SIZES[0]);
    wait_until(&mut window, |app| app.snapshot().node.mining && app.snapshot().full_rate > 0);

    retype(&mut window, "Exact limit (H/s)", &LIMIT.to_string());
    press(&mut window, "Apply");
    // The line reads the service's budget, which follows the setting a moment later.
    wait_until(&mut window, |app| app.settings().hash_cap == LIMIT && app.snapshot().budget.limit == LIMIT && app.snapshot().node.hashrate > 0);

    // Basic Korean: the line by the button says what a new miner starts at, then it starts there.
    speak(&mut window, Mode::Basic, Lang::Ko);
    window.state_mut().open("page.mining");
    window.run_steps(SETTLE_STEPS);
    let half = format::number((LIMIT / 2).into());
    let said = texts_with(&window, "새 채굴기");
    press(&mut window, "채굴기 더하기");
    wait_for_miners(&mut window, 1);
    let (most, mean) = watch(&mut window, 30);
    println!("one added miner: highest {most}, mean {mean} against {LIMIT}");
    assert!(most as f64 <= LIMIT as f64 * READING_SLACK, "together {most} went over {LIMIT}");
    assert!(mean as f64 <= LIMIT as f64 * MEAN_SLACK, "together {mean} on average, over {LIMIT}");
    assert!(said.iter().any(|text| text.contains(&half)), "the start is said before the miner starts: {said:?}");
    let snap = window.state().snapshot();
    let child_cap = snap.local[0].status.as_ref().map_or(0, |status| status.cap);
    assert_eq!((snap.node.cap, child_cap), (LIMIT / 2, LIMIT / 2), "this app and the miner share the limit");
    window.state_mut().open("page.home");
    window.run_steps(SETTLE_STEPS);
    save(&mut window, "budget-home-limit-miner-ko-basic");
    window.state_mut().open("page.mining");
    window.run_steps(SETTLE_STEPS);
    save(&mut window, "budget-mining-limit-miner-ko-basic");

    // Expert English: a second miner, and the three share it.
    speak(&mut window, Mode::Expert, Lang::En);
    press(&mut window, "Add a miner");
    wait_for_miners(&mut window, 2);
    let (most, mean) = watch(&mut window, 30);
    println!("two added miners: highest {most}, mean {mean} against {LIMIT}");
    assert!(most as f64 <= LIMIT as f64 * READING_SLACK, "together {most} went over {LIMIT}");
    assert!(mean as f64 <= LIMIT as f64 * MEAN_SLACK, "together {mean} on average, over {LIMIT}");
    window.state_mut().open("page.home");
    window.run_steps(SETTLE_STEPS);
    save(&mut window, "budget-home-limit-miners-en-expert");

    // No limit: a new miner still starts at a stated share of the computer's highest speed.
    press(&mut window, "Full");
    wait_until(&mut window, |app| app.settings().hash_cap == 0 && app.snapshot().budget.limit == 0);
    window.state_mut().open("page.mining");
    window.run_steps(SETTLE_STEPS);
    let highest = window.state().snapshot().full_rate;
    let said = texts_with(&window, "A new miner");
    assert!(!said.is_empty(), "the start is said before the miner starts");
    press(&mut window, "Add a miner");
    wait_for_miners(&mut window, 3);
    let snap = window.state().snapshot();
    let newest = snap.local.iter().max_by_key(|view| view.spec.id).and_then(|view| view.status.clone()).unwrap();
    assert!(newest.cap > 0 && newest.cap <= highest / 2, "a new miner takes {} of {highest}", newest.cap);
    assert!(said.iter().any(|text| text.contains(&format::number(newest.cap.into()))), "{said:?} against {}", newest.cap);
}

/// The threads a node process runs, where the system says.
fn os_threads(pid: u32) -> Option<usize> {
    olc_service::platform().usage(pid).and_then(|usage| usage.threads)
}

fn settle_threads(window: &mut Window, pid: u32, done: impl Fn(usize) -> bool) -> usize {
    let started = Instant::now();
    loop {
        window.run_steps(1);
        let now = os_threads(pid).unwrap_or(0);
        if done(now) || started.elapsed() > Duration::from_secs(20) {
            return now;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// The highest speed or the limit the speed line stands under, as the snapshot gives them.
fn ceiling(snap: &Snapshot, cap: u64) -> u64 {
    if cap > 0 && cap < snap.full_rate { cap } else { snap.full_rate }
}

/// Waits from a change of speed until the network's speed reads only blocks made after it: the
/// span the Mining page states has passed since the change. Returns how long that took.
fn after_stated_time(window: &mut Window, since: Instant) -> Duration {
    loop {
        window.run_steps(1);
        let span = window.state().snapshot().node.chain.network.map_or(u64::MAX, |reading| reading.span_ms);
        if u64::try_from(since.elapsed().as_millis()).unwrap_or(u64::MAX) >= span {
            window.run_steps(SETTLE_STEPS);
            return since.elapsed();
        }
        assert!(since.elapsed() < Duration::from_secs(240), "the network's speed still reads blocks from before the change: {span} ms");
        std::thread::sleep(Duration::from_millis(250));
    }
}

/// Opens `page` and reads every speed figure on it in one frame against the snapshot it was drawn
/// from: this computer's speed under its ceiling, the network's never below it and, with only this
/// computer mining, not far above it once the stated time has passed, and this computer's share
/// as the one over the other. The snapshot moves on between frames, so it reads until one agrees.
fn figures_agree(window: &mut Window, page: &str, cap: u64) -> String {
    window.state_mut().open(page);
    window.run_steps(SETTLE_STEPS);
    let started = Instant::now();
    loop {
        window.run_steps(1);
        let snap = window.state().snapshot();
        let chain = &snap.node.chain;
        let (here, all) = (chain.hashrate_here, chain.hashrate_all);
        let share = format!("{}%", here * 100 / all.max(1));
        let (here_text, all_text) = (format::rate(voice(window), here), format::rate(voice(window), all));
        let line = texts_with(window, &here_text);
        let seen = chain.network.is_some()
            && !line.is_empty()
            && !texts_with(window, &all_text).is_empty()
            && texts_with(window, &share).contains(&share);
        if seen {
            let top = ceiling(&snap, cap);
            println!("{page}: this computer {here} under {top}, network {all}, share {share}, read from {:?}", chain.network);
            assert!(here as f64 <= top as f64 * 1.02, "{page}: this computer {here} is above its ceiling {top}");
            // Every miner on this coin is on this computer and measured: the network's speed is
            // their measured sum, not a dozen blocks read with a third's scatter, and the share
            // is 100 % with nothing to be off by.
            assert_eq!(chain.network.map(|reading| reading.others), Some(0), "{page}: a block of a miner not measured");
            assert_eq!((all, chain.share()), (here, Some((100, 0))), "{page}: the network is this computer alone");
            return format!("{here_text} · {all_text} · {share}");
        }
        assert!(started.elapsed() < Duration::from_secs(20), "{page} never showed {here_text}, {all_text} and {share} in one frame");
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// The thread cap sets this computer's highest speed and every miner's threads at once, with one
/// thread and with every core; and once the time the Mining page states has passed after a change
/// of threads or of the limit, every speed figure on Home and on Mining agrees.
#[test]
fn the_thread_cap_moves_the_highest_speed_and_every_miner() {
    let mut settings = quiet_settings();
    settings.mine_on_start = true;
    settings.hash_cap = 0;
    settings.port = port_of_scene(66);
    settings.mode = Mode::Expert;
    settings.language = Lang::En;
    let mut window = harness(app(scratch("budget-threads"), settings, Some(node_program())), SIZES[0]);
    wait_until(&mut window, |app| app.snapshot().node.mining && app.snapshot().full_rate > 0);
    let one = window.state().snapshot().full_rate;
    window.state().send(Command::Join { coin: Box::new(quick_coin(one)), address: None });
    wait_in(&mut window, "the coin runs", 2, |snap| snap.node.coin.as_ref().is_some_and(|coin| coin.ticker == "NWT"));
    let spec = NodeSpec { name: "miner-a".into(), memory_mib: 64, disk_mib: 64, mining: true, wallet: WalletChoice::Own, ..Preset::Custom.spec(0) };
    window.state().send(Command::AddNode(Box::new(spec)));
    wait_for_miners(&mut window, 1);
    let pid = window.state().snapshot().local[0].pid.unwrap();
    let Some(alone) = os_threads(pid) else {
        println!("this system does not say how many threads a process runs");
        return;
    };

    let all = cores();
    window.state_mut().open("page.settings");
    window.run_steps(SETTLE_STEPS);
    retype(&mut window, "Mining threads", &all.to_string());
    let widened = Instant::now();
    wait_until(&mut window, |app| app.snapshot().full_rate != one);
    let wide = settle_threads(&mut window, pid, |now| now + 1 >= alone + all);
    println!("miner threads: {alone} on one, {wide} on {all}");
    assert!(wide + 1 >= alone + all, "the added miner mines on every core too: {alone} then {wide}");
    let highest = window.state().snapshot().full_rate;
    window.state_mut().open("page.home");
    window.run_steps(SETTLE_STEPS);
    let shown = format::rate(voice(&window), highest);
    assert!(!texts_with(&window, &shown).is_empty(), "Home shows the highest speed {shown}");
    let waited = after_stated_time(&mut window, widened);
    let (home, mining) = (figures_agree(&mut window, "page.home", 0), figures_agree(&mut window, "page.mining", 0));
    println!("{all} threads, highest {highest} against {one} on one; after {} ms: Home {home}, Mining {mining}", waited.as_millis());

    window.state_mut().open("page.settings");
    window.run_steps(SETTLE_STEPS);
    retype(&mut window, "Mining threads", "1");
    let narrowed = Instant::now();
    wait_until(&mut window, |app| app.snapshot().full_rate == one);
    let narrow = settle_threads(&mut window, pid, |now| now <= alone);
    assert!(narrow <= alone, "back on one thread: {narrow} against {alone}");
    save(&mut window, "budget-threads-one-settings-en-expert");
    window.state_mut().open("page.home");
    window.run_steps(SETTLE_STEPS);
    let shown = format::rate(voice(&window), one);
    assert!(!texts_with(&window, &shown).is_empty(), "Home shows the highest speed {shown} at once");
    let waited = after_stated_time(&mut window, narrowed);
    let (home, mining) = (figures_agree(&mut window, "page.home", 0), figures_agree(&mut window, "page.mining", 0));
    println!("one thread; after {} ms: Home {home}, Mining {mining}", waited.as_millis());
    save(&mut window, "budget-threads-one-mining-en-expert");
    window.state_mut().open("page.home");
    window.run_steps(SETTLE_STEPS);
    save(&mut window, "budget-threads-one-home-en-expert");

    // A limit under the highest: the miners slow to it, and the network's speed follows.
    let limit = one / 2;
    retype(&mut window, "Exact limit (H/s)", &limit.to_string());
    press(&mut window, "Apply");
    let limited = Instant::now();
    wait_until(&mut window, |app| app.settings().hash_cap == limit);
    let waited = after_stated_time(&mut window, limited);
    let (home, mining) = (figures_agree(&mut window, "page.home", limit), figures_agree(&mut window, "page.mining", limit));
    println!("limit {limit}; after {} ms: Home {home}, Mining {mining}", waited.as_millis());
    speak(&mut window, Mode::Basic, Lang::Ko);
    figures_agree(&mut window, "page.home", limit);
    save(&mut window, "budget-threads-limit-home-ko-basic");
    figures_agree(&mut window, "page.mining", limit);
    save(&mut window, "budget-threads-limit-mining-ko-basic");
    // The speed line's limit words at large text, where they most easily meet the line.
    for (mode, lang, tag) in [(Mode::Basic, Lang::Ko, "ko-basic"), (Mode::Expert, Lang::En, "en-expert")] {
        speak(&mut window, mode, lang);
        window.state_mut().settings_mut().text_percent = 150;
        // The size is read through the zoom, so the zoom has to land first. The window is 1280
        // pixels wide, two thirds of the points normal text has, and tall enough to hold the
        // speed line, which large text pushes below the first 800 pixels.
        window.run_steps(2);
        window.set_size(egui::vec2(SIZES[0].0 as f32, 2_400.0) / 1.5);
        for page in ["page.home", "page.mining"] {
            window.state_mut().open(page);
            window.run_steps(SETTLE_STEPS);
            save(&mut window, &format!("budget-threads-limit-{}-{tag}-150", page.trim_start_matches("page.")));
        }
    }
    window.state_mut().settings_mut().text_percent = 100;
    window.run_steps(2);
    window.set_size(egui::vec2(SIZES[0].0 as f32, SIZES[0].1 as f32));
}

/// A coin of two second blocks, so sixty blocks come in about two minutes. At one second a pool's
/// worker spends a large part of each block on work the next block made stale, and that work is in
/// no block, here or in Bitcoin.
const BLOCK_MS: u64 = 2_000;

fn quick_coin(rate: u64) -> olc_core::CoinConfig {
    let now = super::live_swap::unix_ms();
    let values = vec![
        ("name".into(), "Network Test".into()),
        ("ticker".into(), "NWT".into()),
        ("rules.block_time_ms".into(), BLOCK_MS.to_string()),
        ("genesis_time_ms".into(), now.to_string()),
    ];
    let mut coin = olc_store::coin_file::new_coin(&olc_store::coin_file::NewCoin { values }).unwrap();
    coin.initial_difficulty = rate * BLOCK_MS / 1_000;
    coin.rules.min_difficulty = 1;
    coin
}

/// The rows of the breakdown of who found the last blocks: each name and its percent.
fn breakdown(window: &Window) -> Vec<(String, u32)> {
    let mut rows = Vec::new();
    for text in texts_with(window, "%, ") {
        let mut parts = text.splitn(3, ", ");
        let (Some(name), Some(percent)) = (parts.next(), parts.next()) else {
            continue;
        };
        if let Ok(percent) = percent.trim_end_matches('%').parse::<u32>() {
            rows.push((name.to_string(), percent));
        }
    }
    rows
}

/// This app mines, one miner beside it mines alone and another works in the pool this app runs.
/// After fifty blocks the network's speed read from the blocks is within 30 % of what the miners
/// measured, and the breakdown names this computer, its miners and the pool, adding to 100 %.
#[test]
fn the_network_shows_its_speed_and_who_found_the_blocks() {
    const LIMIT: u64 = 30_000;
    let mut settings = quiet_settings();
    settings.mine_on_start = true;
    settings.hash_cap = LIMIT;
    settings.port = port_of_scene(61);
    let mut window = harness(app(scratch("budget-network"), settings, Some(node_program())), SIZES[0]);
    window.state().send(Command::Join { coin: Box::new(quick_coin(LIMIT)), address: None });
    wait_in(&mut window, "the coin runs", 2, |snap| snap.node.coin.as_ref().is_some_and(|coin| coin.ticker == "NWT"));
    for (id, name) in [(0, "miner-a"), (1, "miner-b")] {
        let spec = NodeSpec { name: name.into(), memory_mib: 64, disk_mib: 64, mining: true, ..Preset::Custom.spec(id) };
        window.state().send(Command::AddNode(Box::new(spec)));
    }
    wait_for_miners(&mut window, 2);
    let pool = PoolSettings { port: port_of_scene(62), workers: vec![Who::Node(1)], ..PoolSettings::default() };
    window.state().send(Command::Pool(PoolOrder::Start(pool)));
    wait_in(&mut window, "the pool's worker", 2, |snap| snap.pool.workers.iter().any(|row| row.accepted > 0));
    let from = window.state().snapshot().node.height;
    let started = Instant::now();
    let mut readings = Vec::new();
    while window.state().snapshot().node.height < from + 61 {
        assert!(started.elapsed() < Duration::from_secs(300), "sixty blocks in five minutes");
        window.run_steps(1);
        readings.push(together(&window.state().snapshot()));
        std::thread::sleep(Duration::from_millis(500));
    }
    let measured = readings.iter().sum::<u64>() / readings.len().max(1) as u64;
    let snap = window.state().snapshot();
    // Over the sixty blocks kept, at least the fifty the measure asks for; the twenty block figure
    // beside it moves more with luck.
    let estimate = snap.node.chain.hashrate_average;
    let blocks = snap.node.chain.latest.len() - 1;
    println!("network {estimate} over {blocks} blocks, {} over 20, against {measured} measured over {} readings", snap.node.chain.hashrate_all, readings.len());
    assert!(blocks >= 50, "the average reads {blocks} blocks");
    let off = (estimate as f64 - measured as f64).abs() / measured as f64;
    assert!(off <= 0.30, "the network's speed {estimate} is {:.0} % off the measured {measured}", off * 100.0);

    for (mode, lang, you, pool, name) in [(Mode::Basic, Lang::Ko, "나", "내 풀", "budget-network-ko-basic"), (Mode::Expert, Lang::En, "You", "Your pool", "budget-network-en-expert")] {
        speak(&mut window, mode, lang);
        window.state_mut().open("page.mining");
        window.run_steps(SETTLE_STEPS);
        let rows = breakdown(&window);
        println!("{lang:?}: {rows:?}");
        let names: Vec<&str> = rows.iter().map(|(name, _)| name.as_str()).collect();
        for wanted in [you, "miner-a", pool] {
            assert!(names.contains(&wanted), "the breakdown names {wanted}: {rows:?}");
        }
        assert_eq!(rows.iter().map(|(_, percent)| percent).sum::<u32>(), 100, "the shares add up: {rows:?}");
        // miner-b works in the pool: its blocks are the pool's, so it is named under the pool's
        // row, not as a finder of its own at 0 %.
        assert!(!names.contains(&"miner-b"), "miner-b has no row of its own: {rows:?}");
        let works = if lang == Lang::Ko { "이 풀에서 일함: miner-b" } else { "Working in it: miner-b" };
        let pool_row = texts_with(&window, works);
        assert!(pool_row.iter().any(|row| row.starts_with(pool)), "the pool's row names miner-b: {pool_row:?}");
        let share = if lang == Lang::Ko { "이 컴퓨터 몫" } else { "Hashrate share" };
        assert!(!texts_with(&window, share).is_empty(), "the page says this computer's share");
        // Every miner on this coin is on this computer: its share of the speed is the measured
        // speed over the network's, which reads from the blocks and never below it.
        let mining = figures_agree(&mut window, "page.mining", LIMIT);
        // The long average stands only on Mining, saying how late it follows a change; on Home's
        // small card, beside the speed now and the share, it would read as a contradiction.
        let (average, late) = if lang == Lang::Ko { ("블록 평균", "늦게 따라옴") } else { ("-block average", "Lags a change") };
        assert!(!texts_with(&window, average).is_empty() && !texts_with(&window, late).is_empty(), "Mining shows the average and its lag");
        save(&mut window, name);
        let home = figures_agree(&mut window, "page.home", LIMIT);
        assert!(texts_with(&window, average).is_empty(), "Home leaves the average to Mining: {:?}", texts_with(&window, average));
        save(&mut window, &format!("{name}-home"));
        println!("{lang:?}: Home {home}, Mining {mining}");
    }
}
