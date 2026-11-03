//! The chain pictures on real node processes: this app and two miners on this computer race for
//! blocks, one of them first mines alone and its heavier branch replaces the others' blocks, and
//! every block names who found it. With `OLC_SHOTS` set the widgets are saved as pictures.
use super::{node_program, port_of_scene, quiet_settings, scratch, SETTLE_STEPS};
use crate::core::{unix_ms, Command, Core, CoreLaunch, Miner, Snapshot};
use crate::i18n::{Lang, Mode, Voice};
use crate::local::{is_running, NodeSpec, Preset};
use crate::settings::{Paths, Settings};
use crate::ui::chain::{self, Look, Strip};
use crate::ui::{fonts, paint, theme, tokens, widgets};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use olc_core::CoinConfig;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

const BLOCK_MS: u64 = 2_000;
const APP_CAP: u64 = 3_000;
/// Miner A mines alone at this speed first, so its branch outweighs the others' quickly.
const ALONE_CAP: u64 = 12_000;
const A_CAP: u64 = 6_000;
const B_CAP: u64 = 2_000;
const A: u32 = 46;
const B: u32 = 47;
/// Intervals the estimate is checked against.
const MEASURED: usize = 24;

fn miner(id: u32, name: &str, cap: u64, joined: bool) -> NodeSpec {
    // A small disk opens at once after a restart, so Miner A's branch is still the heavier one when
    // it joins; a first opening reads every sector of the room it is given.
    NodeSpec { name: name.into(), mining: true, hash_cap: cap, joined, disk_mib: 16, ..Preset::Custom.spec(id) }
}

#[track_caller]
fn wait(what: &str, seconds: u64, mut done: impl FnMut() -> bool) {
    let start = Instant::now();
    while !done() {
        assert!(start.elapsed() < Duration::from_secs(seconds), "waited {seconds} s for {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// The widgets as a page would hold them, on the app's own background.
struct Gallery {
    snap: Arc<Snapshot>,
    dark: bool,
    voice: Voice,
    fonts_set: bool,
}

fn draw(ui: &mut egui::Ui, gallery: &mut Gallery) {
    let ctx = ui.ctx().clone();
    if !gallery.fonts_set {
        gallery.fonts_set = true;
        ctx.set_fonts(fonts::definitions());
        ctx.request_repaint();
        return;
    }
    let t = if gallery.dark { tokens::DARK } else { tokens::LIGHT };
    theme::apply(&ctx, &t);
    let whole = ctx.content_rect();
    ui.painter().add(paint::lit_back(whole, &t));
    let node = &gallery.snap.node;
    let ticker = node.coin.as_ref().map(|coin| coin.ticker.clone()).unwrap_or_default();
    let look = Look { t: &t, voice: gallery.voice, still: false, now_ms: unix_ms(), ticker: &ticker };
    let chain = &node.chain;
    ui.scope_builder(egui::UiBuilder::new().max_rect(whole.shrink(32.0)), |ui| {
        chain::block_strip(ui, &look, &Strip::of(chain));
        ui.add_space(tokens::GAP_WIDE);
        ui.columns(2, |columns| {
            widgets::card(&mut columns[0], &t, |ui| chain::race_bars(ui, &look, &chain.shares));
            widgets::card(&mut columns[1], &t, |ui| {
                chain::hashrate_line(ui, &look, chain, 150.0);
                ui.add_space(tokens::GAP_WIDE);
                chain::next_block_bar(ui, &look, chain);
            });
        });
    });
}

type Window = Harness<'static, Gallery>;

fn gallery(snap: Arc<Snapshot>) -> Window {
    let state = Gallery { snap, dark: true, voice: Voice { lang: Lang::En, mode: Mode::Basic }, fonts_set: false };
    let mut window = Harness::builder().with_size(egui::vec2(1280.0, 800.0)).with_step_dt(0.05).build_ui_state(draw, state);
    window.run_steps(SETTLE_STEPS);
    window
}

fn shoot(window: &mut Window, name: &str) {
    let Ok(folder) = std::env::var("OLC_SHOTS") else {
        return;
    };
    for (dark, lang, mode, look) in [
        (true, Lang::Ko, Mode::Basic, "dark-ko"),
        (false, Lang::Ko, Mode::Basic, "light-ko"),
        (true, Lang::En, Mode::Expert, "dark-en-expert"),
    ] {
        window.state_mut().dark = dark;
        window.state_mut().voice = Voice { lang, mode };
        window.run_steps(SETTLE_STEPS);
        let image = window.render().expect("a renderer");
        std::fs::create_dir_all(&folder).expect("the picture folder");
        image.save(Path::new(&folder).join(format!("chain-{name}-{look}.png"))).expect("save");
    }
    window.state_mut().voice = Voice { lang: Lang::En, mode: Mode::Basic };
    window.run_steps(SETTLE_STEPS);
}

fn pid_of(core: &Core, id: u32) -> Option<u32> {
    core.snapshot().local.iter().find(|view| view.spec.id == id).and_then(|view| view.pid)
}

/// Stops a miner and starts it again with `spec`, once the old process is gone.
fn restart(core: &Core, spec: NodeSpec) {
    let pid = pid_of(core, spec.id);
    core.send(Command::RemoveNode(spec.id));
    if let Some(pid) = pid {
        wait("the miner to stop", 30, || !is_running(pid));
    }
    core.send(Command::AddNode(Box::new(spec)));
}

fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}

/// The old line on Home divided by this computer's speed alone; the new one by every miner's.
/// Each guess is the one shown when the block before was found, against the time it really took.
fn measure_the_wait(core: &Core) {
    let mut last = core.snapshot().node.height;
    let mut guesses: Option<(u64, u64, u64)> = None;
    let (mut old_errors, mut new_errors, mut real) = (Vec::new(), Vec::new(), Vec::new());
    let (mut old_sum, mut new_sum) = (0.0, 0.0);
    wait("blocks to measure the wait", 300, || {
        let snap = core.snapshot();
        let node = &snap.node;
        if node.height == last {
            return false;
        }
        let single = node.height == last + 1;
        last = node.height;
        let newest = &node.chain.latest;
        if let (true, Some((old, new, previous)), Some(block)) = (single, guesses, newest.first()) {
            let took = block.time_ms.saturating_sub(previous) as f64;
            if took > 0.0 {
                old_errors.push((old as f64 - took).abs() / old as f64);
                new_errors.push((new as f64 - took).abs() / new as f64);
                real.push(took);
                old_sum += old as f64;
                new_sum += new as f64;
            }
        }
        let old = node.seconds_per_block().map(|_| node.difficulty * 1_000 / node.hashrate.max(1));
        guesses = match (old, node.chain.expected_ms, newest.first()) {
            (Some(old), Some(new), Some(block)) => Some((old, new, block.time_ms)),
            _ => None,
        };
        real.len() >= MEASURED
    });
    let count = real.len() as f64;
    let mean_real = real.iter().sum::<f64>() / count;
    eprintln!(
        "wait over {} blocks: real mean {:.0} ms, median {:.0} ms | this computer alone: mean guess {:.0} ms, median error {:.0}% | every miner: mean guess {:.0} ms, median error {:.0}% | a perfect guess of an exponential wait errs by 64% at the median",
        real.len(),
        mean_real,
        median(real.clone()),
        old_sum / count,
        median(old_errors) * 100.0,
        new_sum / count,
        median(new_errors.clone()) * 100.0,
    );
    let mean_guess = new_sum / count;
    assert!(mean_guess > mean_real / 3.0 && mean_guess < mean_real * 3.0, "the guess {mean_guess:.0} ms is far from {mean_real:.0} ms");
}

#[test]
fn the_chain_shows_who_mined_each_block_and_the_race() {
    let root = scratch("chain-shots");
    let settings = Settings { port: port_of_scene(50), mine_on_start: true, ..quiet_settings() };
    let launch = CoreLaunch { paths: Paths { root }, fleet: None, usb: false, node_program: Some(node_program()) };
    let core = Core::start(launch, settings).unwrap();
    let mut coin = CoinConfig::olc();
    coin.name = "Race".into();
    coin.ticker = "RCE".into();
    coin.rules.block_time_ms = BLOCK_MS;
    coin.rules.min_difficulty = 100;
    coin.initial_difficulty = (APP_CAP + B_CAP) * BLOCK_MS / 1_000;
    core.send(Command::Join { coin: Box::new(coin), address: None });
    core.send(Command::SetCap(APP_CAP));
    core.send(Command::SetMining(true));
    // Miner A mines alone from the start, Miner B with this app.
    core.send(Command::AddNode(Box::new(miner(A, "Miner A", ALONE_CAP, false))));
    core.send(Command::AddNode(Box::new(miner(B, "Miner B", B_CAP, true))));
    let status_of = |id: u32| core.snapshot().local.iter().find(|view| view.spec.id == id).and_then(|view| view.status.clone());
    wait("both sides to mine", 240, || {
        let node_height = core.snapshot().node.height;
        let alone = status_of(A).map_or(0, |status| status.height);
        node_height >= 3 && alone >= node_height + 3
    });

    // Miner A joins: its heavier branch replaces what this app and Miner B found meanwhile.
    restart(&core, miner(A, "Miner A", ALONE_CAP, true));
    // Miner A forked at the first block, so the branch that lost starts there. A race between this
    // app and Miner B before it would be kept the same way, so this waits for the one from 1.
    // The view it was seen in is kept: a small race right after would replace it in the next one.
    // The strip draws an open race in place of the last reorg, so the view kept has none open.
    let mut seen = None;
    wait("the lost blocks", 120, || {
        let snap = core.snapshot();
        let chain = &snap.node.chain;
        let from_one = chain.replaced.as_ref().is_some_and(|lost| lost.first() == 1);
        if from_one && chain.rival.is_none() {
            seen = Some(snap);
        }
        seen.is_some()
    });
    let snap = seen.expect("a view with the lost blocks");
    let replaced = snap.node.chain.replaced.clone().expect("a reorg");
    let other_side = |miner: &Miner| *miner == Miner::You || *miner == Miner::Here("Miner B".into());
    assert!(replaced.blocks.iter().all(|block| other_side(&block.miner)), "{replaced:?}");
    let mut window = gallery(snap);
    let said = window.query_all_by_label_contains("lost the race").next().is_some();
    assert!(said, "the strip says which blocks lost");
    // Eleven blocks fit across: a busy machine may let Miner A run further ahead than that.
    if window.state().snap.node.height < 10 {
        let first = format!("Block {}, ", replaced.first());
        assert!(window.query_all_by_label_contains(&first).next().is_some(), "the lost blocks are drawn");
    }
    shoot(&mut window, "replaced");

    // All three race on one chain at the speeds a person would set.
    core.send(Command::NodeCap { id: A, cap: A_CAP });
    measure_the_wait(&core);
    wait("a strip of blocks", 240, || core.snapshot().node.chain.latest.len() >= 12);
    let snap = core.snapshot();
    let chain = &snap.node.chain;
    let named = |miner: &Miner| matches!(miner, Miner::You) || *miner == Miner::Here("Miner A".into()) || *miner == Miner::Here("Miner B".into());
    assert!(chain.latest.iter().all(|block| named(&block.miner)), "a block of this computer went unnamed");
    let miners: std::collections::BTreeSet<_> = chain.latest.iter().map(|block| block.miner.clone()).collect();
    assert!(miners.len() >= 2, "{miners:?}");
    assert_eq!(chain.window() as usize, chain.latest.len());
    assert!(chain.hashrate_here > 0 && !chain.trail.is_empty());
    assert!(chain.expected_ms.is_some() && chain.average_ms.is_some());
    window.state_mut().snap = snap.clone();
    window.run_steps(SETTLE_STEPS);
    assert!(window.query_all_by_label_contains("Miner A").next().is_some());
    assert!(window.query_all_by_label_contains("Next block").next().is_some());
    shoot(&mut window, "race");

    // The next block slides in from the left: one picture halfway.
    let height = snap.node.height;
    // A reorg or two quick blocks can step past one more, so any later height will do.
    wait("one more block", 120, || core.snapshot().node.height > height);
    window.state_mut().snap = core.snapshot();
    window.run_steps(3);
    if let Ok(folder) = std::env::var("OLC_SHOTS") {
        let image = window.render().expect("a renderer");
        image.save(Path::new(&folder).join("chain-slide.png")).expect("save");
    }
    core.send(Command::RemoveNode(A));
    core.send(Command::RemoveNode(B));
}
