//! The Lab on this computer. It starts plain miners of its own, real node processes on a coin of
//! its own, sets their caps to the share a person chose, and runs olc-lab's double spend or split
//! on them through the lines a board answers. Its own coin keeps the person's chain and wallet
//! out of it: a reorganisation here replaces lab blocks only, and nobody else's node is touched.
//! The miners go when the run ends, unless the person keeps them; kept ones are used again.
use super::lab_picture::{replaced_between, Sides};
use super::supervisor::LocalView;
use super::{Holding, NodeSpec, Preset, Supervisor, WalletChoice};
use crate::settings::Paths;
use olc_board::BoardLink;
use olc_core::coin::COIN;
use olc_core::{CoinConfig, Rules};
use olc_lab::{AttackOptions, Journal, Outcome, SplitOptions, Step, Watch};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

pub use super::lab_picture::{Frame, Mark, Picture, Row, Side};

/// Expected hashes for one block. With [`TOTAL_RATE`] a block comes about every two seconds, slow
/// enough to watch and quick enough to end a run in a minute or two.
const DIFFICULTY: u64 = 48_000;
/// What all lab miners hash together, split by the chosen share.
pub const TOTAL_RATE: u64 = 24_000;
const FIRST_ID: u32 = 900;
const READY_WAIT: Duration = Duration::from_secs(60);
const LOOK: Duration = Duration::from_millis(400);
const MEMORY_MIB: u64 = 64;
const DISK_MIB: u64 = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LabKind {
    Attack,
    Split,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LabChoice {
    pub kind: LabKind,
    /// The attacker's (or left group's) percent of all hashing, 10 to 90.
    pub share: u8,
    /// A second miner hashes with the attacker.
    pub helper: bool,
    /// Coins the attacker pays the victim.
    pub amount: u64,
    pub confirmations: u64,
    /// Keeps the miners running after the run, to be used again.
    pub keep: bool,
    /// What all miners hash together.
    pub total_rate: u64,
}

impl Default for LabChoice {
    fn default() -> Self {
        Self { kind: LabKind::Attack, share: 60, helper: false, amount: 10, confirmations: 2, keep: false, total_rate: TOTAL_RATE }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Phase {
    #[default]
    Idle,
    /// Miners are starting.
    Starting,
    /// The attacker mines coins it can spend.
    Warming,
    Running,
    Done,
    Failed,
}

#[derive(Clone, Debug, Default)]
pub struct LabState {
    pub phase: Phase,
    pub kind: Option<LabKind>,
    pub frames: Vec<Frame>,
    pub live: Picture,
    pub lines: Vec<String>,
    pub outcome: Option<Outcome>,
    pub error: Option<String>,
    /// Miners kept from an earlier run, still running.
    pub kept: bool,
    /// Each miner as it stands now.
    pub miners: Vec<MinerRow>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MinerRow {
    pub name: String,
    pub height: u64,
    pub peers: usize,
    pub rate: u64,
    pub cap: u64,
    pub offline: bool,
    pub pid: Option<u32>,
}

fn miner_rows(miners: &mut Miners) -> Vec<MinerRow> {
    views(miners)
        .into_iter()
        .map(|view| {
            let status = view.status.clone().unwrap_or_default();
            MinerRow {
                name: view.spec.name.clone(),
                height: status.height,
                peers: status.peers,
                rate: status.hashrate,
                cap: status.cap,
                offline: status.offline,
                pid: view.pid,
            }
        })
        .collect()
}

/// The program and folder for lab miners. The app's own program and data folder unless a test
/// names others.
static PLACE: OnceLock<Mutex<Option<(PathBuf, PathBuf)>>> = OnceLock::new();

/// For runs outside the installed app, such as tests: the node program and a folder of their own.
pub fn use_place(program: PathBuf, folder: PathBuf) {
    if let Ok(mut place) = PLACE.get_or_init(Mutex::default).lock() {
        *place = Some((program, folder));
    }
}

fn place() -> Result<(PathBuf, PathBuf), String> {
    let chosen = PLACE.get_or_init(Mutex::default).lock().ok().and_then(|place| place.clone());
    match chosen {
        Some(place) => Ok(place),
        None => {
            let program = std::env::current_exe().map_err(|error| error.to_string())?;
            Ok((program, crate::settings::standard_folder()?.join("lab")))
        }
    }
}

/// Miners and whose they are.
struct Miners {
    supervisor: Supervisor,
    kind: LabKind,
    helper: bool,
    /// Attacker, victim, helper; or left, left, right.
    specs: Vec<NodeSpec>,
}

/// Miners a person kept, between runs.
static KEPT: Mutex<Option<Miners>> = Mutex::new(None);

pub fn has_kept() -> bool {
    KEPT.lock().is_ok_and(|kept| kept.is_some())
}

/// Stops kept miners and removes their folder.
pub fn remove_kept() {
    let taken = KEPT.lock().ok().and_then(|mut kept| kept.take());
    if let Some(mut miners) = taken {
        miners.supervisor.stop_all();
        if let Ok((_, folder)) = place() {
            remove_later(folder);
        }
    }
}

/// A stopped node may write for a moment longer, so its folder goes once it surely ended.
fn remove_later(folder: PathBuf) {
    thread::spawn(move || {
        thread::sleep(Duration::from_secs(3));
        let _ = std::fs::remove_dir_all(folder);
    });
}

fn lab_coin() -> CoinConfig {
    CoinConfig {
        name: "OLC Lab".into(),
        ticker: "LAB".into(),
        message: "OLC Lab, this computer only".into(),
        genesis_time_ms: crate::core::unix_ms(),
        initial_difficulty: DIFFICULTY,
        premine: Vec::new(),
        rules: Rules {
            block_time_ms: 2_000,
            retarget_interval: 0,
            reward: 50 * COIN,
            coinbase_maturity: 2,
            min_difficulty: DIFFICULTY,
            floor_from_height: 0,
            ..Rules::default()
        },
        upgrade_from: 0,
        ..CoinConfig::olc()
    }
}

fn spec(id: u32, name: &str) -> NodeSpec {
    NodeSpec {
        name: name.into(),
        preset: Preset::Custom,
        hash_cap: 0,
        memory_mib: MEMORY_MIB,
        cpu_percent: 0,
        disk_mib: DISK_MIB,
        wallet: WalletChoice::Own,
        joined: true,
        listen_lan: false,
        mining: false,
        holding: Holding::Everything,
        ..Preset::Custom.spec(id)
    }
}

fn names(choice: &LabChoice) -> Vec<&'static str> {
    match (choice.kind, choice.helper) {
        (LabKind::Attack, false) => vec!["lab-attacker", "lab-victim"],
        (LabKind::Attack, true) => vec!["lab-attacker", "lab-victim", "lab-helper"],
        (LabKind::Split, _) => vec!["lab-left-1", "lab-left-2", "lab-right"],
    }
}

/// Caps by spec order, so the strong side holds `share` percent of `total`.
pub fn caps(choice: &LabChoice) -> Vec<u64> {
    let share = u64::from(choice.share.clamp(10, 90));
    let strong = choice.total_rate * share / 100;
    let other = choice.total_rate - strong;
    match (choice.kind, choice.helper) {
        (LabKind::Attack, false) => vec![strong, other],
        // The helper brings a third of the attacker's side.
        (LabKind::Attack, true) => vec![strong - strong / 3, other, strong / 3],
        (LabKind::Split, _) => vec![strong - strong / 2, strong / 2, other],
    }
}

pub struct LabRun {
    state: Arc<Mutex<LabState>>,
    stop: Arc<AtomicBool>,
}

impl LabRun {
    pub fn start(choice: LabChoice) -> Self {
        let state = Arc::new(Mutex::new(LabState { phase: Phase::Starting, kind: Some(choice.kind), ..LabState::default() }));
        let stop = Arc::new(AtomicBool::new(false));
        let (shared, flag) = (state.clone(), stop.clone());
        thread::spawn(move || run(&choice, &shared, &flag));
        Self { state, stop }
    }

    pub fn state(&self) -> LabState {
        self.state.lock().map(|state| state.clone()).unwrap_or_default()
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    pub fn running(&self) -> bool {
        matches!(self.state().phase, Phase::Starting | Phase::Warming | Phase::Running)
    }
}

fn update(state: &Mutex<LabState>, change: impl FnOnce(&mut LabState)) {
    if let Ok(mut state) = state.lock() {
        change(&mut state);
    }
}

fn run(choice: &LabChoice, state: &Arc<Mutex<LabState>>, stop: &Arc<AtomicBool>) {
    let result = prepare(choice, state, stop).map(|miners| {
        let miners = Arc::new(Mutex::new(miners));
        let result = experiment(choice, &miners, state, stop);
        (miners, result)
    });
    let (miners, result) = match result {
        Ok((miners, result)) => (Some(miners), result),
        Err(error) => (None, Err(error)),
    };
    let kept = choice.keep && result.is_ok();
    match miners.map(Arc::try_unwrap) {
        Some(Ok(miners)) if kept => {
            if let (Ok(miners), Ok(mut slot)) = (miners.into_inner(), KEPT.lock()) {
                *slot = Some(miners);
            }
        }
        Some(Ok(miners)) => end(&miners),
        // Something still holds them, which should not happen: they stop all the same.
        Some(Err(shared)) => end(&shared),
        None => {}
    }
    update(state, |state| {
        state.kept = kept;
        match result {
            Ok(outcome) => {
                state.phase = Phase::Done;
                state.outcome = outcome;
            }
            Err(error) => {
                state.phase = Phase::Failed;
                state.error = Some(error);
            }
        }
    });
}

/// Stops every miner and removes the lab's folder.
fn end(miners: &Mutex<Miners>) {
    if let Ok(mut miners) = miners.lock() {
        miners.supervisor.stop_all();
    }
    if let Ok((_, folder)) = place() {
        remove_later(folder);
    }
}

/// Kept miners of the same shape, or new ones on a new lab coin.
fn prepare(choice: &LabChoice, state: &Mutex<LabState>, stop: &AtomicBool) -> Result<Miners, String> {
    let kept = KEPT.lock().ok().and_then(|mut kept| kept.take());
    let mut miners = match kept {
        Some(miners) if miners.kind == choice.kind && miners.helper == choice.helper => miners,
        other => {
            if let Some(mut old) = other {
                old.supervisor.stop_all();
                thread::sleep(Duration::from_secs(3));
            }
            start_miners(choice, stop)?
        }
    };
    for (id, cap) in ids(&miners).into_iter().zip(caps(choice)) {
        miners.supervisor.send(id, &format!("board limit {cap}"))?;
        // From the first line on, the node says its chain in every status line.
        miners.supervisor.send(id, "board status")?;
    }
    if choice.kind == LabKind::Attack {
        update(state, |state| state.phase = Phase::Warming);
        warm_up(choice, &mut miners, state, stop)?;
    }
    for id in ids(&miners) {
        miners.supervisor.send(id, "board mine on")?;
    }
    for (id, cap) in ids(&miners).into_iter().zip(caps(choice)) {
        miners.supervisor.send(id, &format!("board limit {cap}"))?;
    }
    Ok(miners)
}

fn start_miners(choice: &LabChoice, stop: &AtomicBool) -> Result<Miners, String> {
    let (program, folder) = place()?;
    let _ = std::fs::remove_dir_all(&folder);
    let coin_folder = folder.join("coins").join("olc-lab");
    std::fs::create_dir_all(&coin_folder).map_err(|error| error.to_string())?;
    let coin = coin_folder.join("coin.json");
    std::fs::write(&coin, olc_store::coin_file::to_json(&lab_coin())?).map_err(|error| error.to_string())?;
    let mut supervisor = Supervisor::with_program(Paths { root: folder }, program);
    let specs: Vec<NodeSpec> =
        names(choice).iter().enumerate().map(|(at, name)| spec(FIRST_ID + at as u32, name)).collect();
    // The first miner is the hub. The others dial the port it really took.
    supervisor.start(&specs[0], &coin, &[])?;
    let hub = wait_for(&mut supervisor, &specs[0], stop, |view| {
        view.status.as_ref().and_then(|status| status.port).map(|port| format!("127.0.0.1:{port}"))
    })?;
    for spec in &specs[1..] {
        supervisor.start(spec, &coin, std::slice::from_ref(&hub))?;
    }
    for spec in &specs {
        wait_for(&mut supervisor, spec, stop, |view| view.status.as_ref().map(|_| ()))?;
    }
    Ok(Miners { supervisor, kind: choice.kind, helper: choice.helper, specs })
}

fn wait_for<T>(
    supervisor: &mut Supervisor,
    spec: &NodeSpec,
    stop: &AtomicBool,
    ready: impl Fn(&LocalView) -> Option<T>,
) -> Result<T, String> {
    let until = Instant::now() + READY_WAIT;
    loop {
        supervisor.reap();
        let view = supervisor.view(spec);
        if let Some(found) = ready(&view) {
            return Ok(found);
        }
        if let Some(ended) = view.ended {
            return Err(format!("{} ended: {ended}", spec.name));
        }
        if stop.load(Ordering::Relaxed) {
            return Err("stopped by hand.".into());
        }
        if Instant::now() > until {
            return Err(format!("{} did not start.", spec.name));
        }
        thread::sleep(LOOK);
    }
}

fn ids(miners: &Miners) -> Vec<u32> {
    miners.specs.iter().map(|spec| spec.id).collect()
}

fn views(miners: &mut Miners) -> Vec<LocalView> {
    miners.specs.clone().iter().map(|spec| miners.supervisor.view(spec)).collect()
}

/// The attacker mines alone at the whole rate until it holds what it pays, twice over.
fn warm_up(choice: &LabChoice, miners: &mut Miners, state: &Mutex<LabState>, stop: &AtomicBool) -> Result<(), String> {
    let attacker = miners.specs[0].id;
    for id in ids(miners) {
        miners.supervisor.send(id, "board mine off")?;
    }
    miners.supervisor.send(attacker, &format!("board limit {}", choice.total_rate))?;
    miners.supervisor.send(attacker, "board mine on")?;
    let needed = choice.amount * COIN * 2;
    let until = Instant::now() + READY_WAIT * 3;
    loop {
        let all = views(miners);
        let spendable = all.first().and_then(|view| view.status.as_ref()).map_or(0, |status| status.spendable);
        let caught_up = all.iter().all(|view| view.status.as_ref().map(|status| &status.tip) == all[0].status.as_ref().map(|status| &status.tip));
        update(state, |state| state.live = picture(choice, &all, &Sides::default()));
        if spendable >= needed && caught_up {
            return Ok(());
        }
        if stop.load(Ordering::Relaxed) {
            return Err("stopped by hand.".into());
        }
        if Instant::now() > until {
            return Err("the attacker found no coins to spend.".into());
        }
        thread::sleep(LOOK);
    }
}

/// The picture from what each miner says now. Attack: public chain is the victim's, hidden the
/// attacker's. Split: the left group's chain above, the right one's below.
fn picture(choice: &LabChoice, all: &[LocalView], sides: &Sides) -> Picture {
    let total: u64 = all.iter().filter_map(|view| view.status.as_ref()).map(|status| status.hashrate).sum();
    let get = |at: usize| all.get(at);
    let pick = |ats: &[usize]| ats.iter().filter_map(|at| get(*at)).collect::<Vec<_>>();
    let (upper, upper_side, lower, lower_side) = match choice.kind {
        LabKind::Attack => (1, pick(&[1]), 0, pick(&[0, 2])),
        LabKind::Split => (0, pick(&[0, 1]), 2, pick(&[2])),
    };
    Picture {
        upper: sides.row(get(upper), &upper_side, total),
        lower: sides.row(get(lower), &lower_side, total),
        replaced: Vec::new(),
    }
}

fn sides_of(choice: &LabChoice, all: &[LocalView]) -> Sides {
    let address = |at: usize| all.get(at).and_then(|view| view.status.as_ref()).map(|status| status.address.clone());
    let strong: Vec<usize> = match choice.kind {
        LabKind::Attack => vec![0, 2],
        LabKind::Split => vec![0, 1],
    };
    let other: Vec<usize> = match choice.kind {
        LabKind::Attack => vec![1],
        LabKind::Split => vec![2],
    };
    Sides {
        strong: strong.into_iter().filter_map(address).collect(),
        other: other.into_iter().filter_map(address).collect(),
        payment: None,
        spend: None,
    }
}

/// Runs the chosen experiment, drawing the chains on every step and between steps.
fn experiment(
    choice: &LabChoice,
    miners: &Arc<Mutex<Miners>>,
    state: &Arc<Mutex<LabState>>,
    stop: &Arc<AtomicBool>,
) -> Result<Option<Outcome>, String> {
    let (links, all) = {
        let mut held = miners.lock().map_err(|_| "The lab stopped.".to_string())?;
        let links: Vec<Box<dyn BoardLink>> = ids(&held)
            .into_iter()
            .filter_map(|id| held.supervisor.link(id))
            .map(|link| Box::new(link) as Box<dyn BoardLink>)
            .collect();
        (links, views(&mut held))
    };
    let sides = Arc::new(Mutex::new(sides_of(choice, &all)));
    // The public row as it stood before the reveal, to find the blocks it lost.
    let before: Arc<Mutex<Option<Picture>>> = Arc::default();
    let draw = {
        let (choice, miners, sides, before) = (choice.clone(), miners.clone(), sides.clone(), before.clone());
        move || -> Picture {
            let all = miners.lock().map(|mut miners| views(&mut miners)).unwrap_or_default();
            let sides = sides.lock().map(|sides| sides.clone()).unwrap_or_default();
            let mut drawn = picture(&choice, &all, &sides);
            if let Some(before) = before.lock().ok().and_then(|before| before.clone()) {
                let mut lost = replaced_between(&before.upper, &drawn.upper);
                lost.extend(replaced_between(&before.lower, &drawn.lower));
                lost.sort_by_key(|mark| (mark.height, mark.id.clone()));
                lost.dedup();
                drawn.replaced = lost;
            }
            drawn
        }
    };
    let draw = Arc::new(draw);
    let say = {
        let state = state.clone();
        Arc::new(move |line: String| update(&state, |state| state.lines.push(line)))
    };
    let watch: Watch = {
        let (state, draw, sides, before) = (state.clone(), draw.clone(), sides.clone(), before.clone());
        Arc::new(move |step: Step| {
            if let Ok(mut sides) = sides.lock() {
                match &step {
                    Step::Paid { payment } => sides.payment = Some(payment.clone()),
                    Step::SpentAgain { spend } => sides.spend = Some(spend.clone()),
                    _ => {}
                }
            }
            if matches!(step, Step::Revealed | Step::Rejoined) {
                // Drawn before the slot is taken: drawing reads the slot.
                let drawn = draw();
                if let Ok(mut before) = before.lock() {
                    *before = Some(drawn);
                }
            }
            let picture = draw();
            update(&state, |state| state.frames.push(Frame { step, picture }));
        })
    };
    update(state, |state| state.phase = Phase::Running);
    let watching = Arc::new(AtomicBool::new(true));
    let looker = {
        let (state, draw, watching, miners) = (state.clone(), draw.clone(), watching.clone(), miners.clone());
        thread::spawn(move || {
            while watching.load(Ordering::Relaxed) {
                let picture = draw();
                let rows = miners.lock().map(|mut miners| miner_rows(&mut miners)).unwrap_or_default();
                update(&state, |state| {
                    state.live = picture;
                    state.miners = rows;
                });
                thread::sleep(LOOK);
            }
        })
    };
    let (_, folder) = place()?;
    let result = match choice.kind {
        LabKind::Attack => olc_lab::attack_with(
            links,
            AttackOptions {
                amount: choice.amount.to_string(),
                confirmations: choice.confirmations,
                journal: Journal::at(folder.join("journal")),
                timeout: Duration::from_secs(900),
                give_up_behind: 6,
                race_limit: Duration::from_secs(180),
            },
            say,
            watch,
            stop.clone(),
        )
        .map(Some),
        LabKind::Split => {
            let mut links = links;
            let right = links.pop().ok_or("The right group needs a miner.")?;
            let options = SplitOptions { apart_blocks: 3, apart_limit: Duration::from_secs(90), timeout: Duration::from_secs(600) };
            olc_lab::split_with(links, right, options, say, watch, stop.clone()).map(|()| None)
        }
    };
    watching.store(false, Ordering::Relaxed);
    let _ = looker.join();
    let last = draw();
    update(state, |state| state.live = last);
    result
}
