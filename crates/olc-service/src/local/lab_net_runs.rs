//! Experiments on the network's own participants: the double spend and the split of olc-lab,
//! with the same steps, words and pictures the Lab always showed, and the bug exploiter's tricks.
//! Each runs on its own thread through the lines a board answers; the keeper draws its picture.
use super::*;
use crate::local::lab_picture::{replaced_between, Frame, Sides};
use crate::core::Verdict;
use crate::local::trouble::{try_trick, Outcome as TrickOutcome, Trick};
use std::net::SocketAddr;
use crate::local::LocalView;
use olc_board::BoardLink;
use super::roles::ATTACK_AMOUNT;

type Say = Arc<dyn Fn(String) + Send + Sync>;
use olc_core::CoinConfig;
use olc_lab::{AttackOptions, Journal, SplitOptions, Step, Watch};
use std::sync::atomic::{AtomicBool, Ordering};

/// Once the first miner is this far ahead, the blocks are made even if a slow node lags.
const MAKE_LAG: Duration = Duration::from_secs(30);
/// Tries of one trick on one node before what it got is said.
const TRICK_TRIES: usize = 4;
const TRICK_AGAIN: Duration = Duration::from_secs(2);

struct Run {
    kind: LabKind,
    /// Attacker, victim, helper; or the left group then the right miner.
    ids: Vec<u32>,
    /// Honest miners beside the victim.
    public: Vec<u32>,
    state: Arc<Mutex<LabState>>,
    sides: Arc<Mutex<Sides>>,
    live: Arc<Mutex<Picture>>,
    before: Arc<Mutex<Option<Picture>>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

/// What one trick got from one node: its verdict, or why it could not be tried there.
type TrickAnswer = (u32, Trick, Result<TrickOutcome, String>);
/// Each node's newest verdicts, as its status line said them, for the thread that tries tricks.
pub(super) type Verdicts = Arc<Mutex<BTreeMap<u32, Vec<Verdict>>>>;

#[derive(Default)]
pub(super) struct Runs {
    pub experiment: Option<LabState>,
    pub making: Option<(u64, u64)>,
    /// The miner making blocks, the height it started from, and since when.
    pub maker: Option<(u32, u64, Instant)>,
    run: Option<Run>,
    tricks: Vec<(u32, Receiver<TrickAnswer>)>,
    pub verdicts: Verdicts,
}

impl Runs {
    pub fn running(&self) -> bool {
        self.run.is_some()
    }

    pub fn uses(&self, id: u32) -> bool {
        self.run.as_ref().is_some_and(|run| run.ids.contains(&id) || run.public.contains(&id))
    }

    /// Every trick on every target. Each target has a thread of its own that tries the tricks
    /// one after another, since two at once on one node could mix their verdicts; the targets go
    /// side by side, or a network of eight nodes would wait twenty minutes for its last answer.
    /// A busy network can keep a node from answering in time, or move its tip while a trick block
    /// is built on the old one, so a trick with no verdict for its own rule is tried again a few
    /// times; what it got last is what is said.
    pub fn start_tricks(&mut self, exploiter: u32, coin: CoinConfig, targets: Vec<(u32, SocketAddr)>, funds: PathBuf) {
        let (answers, inbox) = channel();
        for (target, address) in targets {
            let (answers, verdicts, coin, funds) = (answers.clone(), Arc::clone(&self.verdicts), coin.clone(), funds.clone());
            thread::spawn(move || {
                // The network runs on while a trick waits: honest blocks and payments the node
                // takes would be read as the trick's verdict, so only refusals are passed on.
                // A trick a node takes then shows as unanswered, never as refused.
                let seen = || {
                    let all = verdicts.lock().ok().and_then(|all| all.get(&target).cloned()).unwrap_or_default();
                    all.into_iter().filter(|verdict| verdict.refused.is_some()).collect()
                };
                for trick in Trick::ALL {
                    let mut outcome = try_trick(trick, &coin, address, &funds, &target.to_string(), &seen);
                    for _ in 1..TRICK_TRIES {
                        if outcome.as_ref().is_ok_and(TrickOutcome::refused_as_expected) {
                            break;
                        }
                        thread::sleep(TRICK_AGAIN);
                        outcome = try_trick(trick, &coin, address, &funds, &target.to_string(), &seen);
                    }
                    if answers.send((target, trick, outcome)).is_err() {
                        return;
                    }
                }
            });
        }
        self.tricks.push((exploiter, inbox));
    }
}

impl Keeper {
    fn link(&self, id: u32) -> Option<Box<dyn BoardLink>> {
        let link = self.supervisor.as_ref()?.link(id)?;
        Some(Box::new(link))
    }

    fn address(&self, id: u32) -> Option<String> {
        self.member(id).and_then(Member::address).map(str::to_string)
    }

    /// An attack with exactly the hashing behind it: the attacker's own, or a pool's `crew`,
    /// against everyone in `public`.
    pub(super) fn start_attack(&mut self, attacker: u32, victim: u32, crew: Vec<u32>, public: Vec<u32>) -> Result<(), String> {
        let links: Vec<Box<dyn BoardLink>> = [attacker, victim].iter().filter_map(|id| self.link(*id)).collect();
        let crew_links: Vec<Box<dyn BoardLink>> = crew.iter().filter_map(|id| self.link(*id)).collect();
        let public_links: Vec<Box<dyn BoardLink>> = public.iter().filter_map(|id| self.link(*id)).collect();
        if links.len() != 2 || crew_links.len() != crew.len() || public_links.len() != public.len() {
            return Err("net.failed".into());
        }
        let attacker_mines = self.member(attacker).is_some_and(|member| member.kind.mines());
        let mut ids = vec![attacker, victim];
        ids.extend(&crew);
        // The pools the honest miners work in are theirs too.
        let pools: Vec<u32> = public.iter().filter_map(|id| self.member(*id)?.pool).collect();
        let sides = Sides {
            strong: [attacker].iter().chain(&crew).filter_map(|id| self.address(*id)).collect(),
            other: public.iter().chain(&pools).chain([&victim]).filter_map(|id| self.address(*id)).collect(),
            payment: None,
            spend: None,
        };
        let journal = self.folder().map(|folder| folder.join("journal")).ok_or("net.failed")?;
        let options = AttackOptions {
            amount: ATTACK_AMOUNT.to_string(),
            confirmations: 2,
            journal: Journal::at(journal),
            timeout: Duration::from_secs(900),
            give_up_behind: 6,
            // Lab blocks come every few seconds: one block of lead loses to a public block
            // found during the reveal about as often as it wins, so it waits for two.
            reveal_lead: 2,
            race_limit: Duration::from_secs(180),
            public: public_links,
            victim_mines: false,
            crew: crew_links,
            attacker_mines,
        };
        self.begin(LabKind::Attack, ids, public, sides, move |say, watch, stop| {
            olc_lab::attack_with(links, options, say, watch, stop).map(Some)
        });
        Ok(())
    }

    pub(super) fn start_split(&mut self, left: Vec<u32>, right: u32) -> Result<(), String> {
        let left_links: Vec<Box<dyn BoardLink>> = left.iter().filter_map(|id| self.link(*id)).collect();
        let right_link = self.link(right).ok_or("net.failed")?;
        if left_links.len() != left.len() {
            return Err("net.failed".into());
        }
        let sides = Sides {
            strong: left.iter().filter_map(|id| self.address(*id)).collect(),
            other: self.address(right).into_iter().collect(),
            payment: None,
            spend: None,
        };
        let mut ids = left;
        ids.push(right);
        // Five blocks a side: with three, chance alone often let the weaker side win.
        let options = SplitOptions { apart_blocks: 5, apart_limit: Duration::from_secs(90), timeout: Duration::from_secs(600) };
        self.begin(LabKind::Split, ids, Vec::new(), sides, move |say, watch, stop| {
            olc_lab::split_with(left_links, right_link, options, say, watch, stop).map(|()| None)
        });
        Ok(())
    }

    fn begin(
        &mut self,
        kind: LabKind,
        ids: Vec<u32>,
        public: Vec<u32>,
        sides: Sides,
        work: impl FnOnce(Say, Watch, Arc<AtomicBool>) -> Result<Option<olc_lab::Outcome>, String> + Send + 'static,
    ) {
        let state = Arc::new(Mutex::new(LabState { phase: Phase::Running, kind: Some(kind), ..LabState::default() }));
        let sides = Arc::new(Mutex::new(sides));
        let live = Arc::new(Mutex::new(Picture::default()));
        let before: Arc<Mutex<Option<Picture>>> = Arc::default();
        let stop = Arc::new(AtomicBool::new(false));
        let say: Say = {
            let state = state.clone();
            Arc::new(move |line: String| {
                if let Ok(mut state) = state.lock() {
                    state.lines.push(line);
                }
            })
        };
        let watch: Watch = {
            let (state, sides, live, before) = (state.clone(), sides.clone(), live.clone(), before.clone());
            Arc::new(move |step: Step| {
                if let Ok(mut sides) = sides.lock() {
                    match &step {
                        Step::Paid { payment } => sides.payment = Some(payment.clone()),
                        Step::SpentAgain { spend } => sides.spend = Some(spend.clone()),
                        _ => {}
                    }
                }
                let picture = live.lock().map(|live| live.clone()).unwrap_or_default();
                if matches!(step, Step::Revealed | Step::Rejoined) {
                    if let Ok(mut before) = before.lock() {
                        *before = Some(picture.clone());
                    }
                }
                if let Ok(mut state) = state.lock() {
                    state.frames.push(Frame { step, picture });
                }
            })
        };
        let thread = {
            let (state, stop) = (state.clone(), stop.clone());
            thread::spawn(move || {
                let result = work(say, watch, stop);
                if let Ok(mut state) = state.lock() {
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
                }
            })
        };
        for member in &mut self.members {
            member.busy = ids.contains(&member.id) || public.contains(&member.id);
        }
        self.runs.run = Some(Run { kind, ids, public, state, sides, live, before, stop, thread: Some(thread) });
    }

    /// Stops the experiment. Its thread puts every node back as it was, or ends with the nodes.
    pub(super) fn stop_run(&mut self) {
        if let Some(run) = self.runs.run.take() {
            run.stop.store(true, Ordering::Relaxed);
            if let Ok(mut state) = run.state.lock() {
                state.phase = Phase::Failed;
                state.error = Some("stopped by hand.".into());
                self.runs.experiment = Some(state.clone());
            }
        }
        for member in &mut self.members {
            member.busy = false;
        }
    }

    pub(super) fn runs_turn(&mut self) {
        self.draw_run();
        self.making_turn();
        self.tricks_turn();
    }

    fn views(&mut self, ids: &[u32]) -> Vec<LocalView> {
        let Some(supervisor) = &mut self.supervisor else {
            return Vec::new();
        };
        ids.iter().filter_map(|id| self.specs.get(id)).map(|spec| supervisor.view(spec)).collect()
    }

    /// The picture as each node says its chain now: the victim's above and the attacker's below,
    /// or the left group's above and the right miner's below.
    fn draw_run(&mut self) {
        let Some(run) = &self.runs.run else {
            return;
        };
        let (kind, ids, public) = (run.kind, run.ids.clone(), run.public.clone());
        let all = self.views(&ids);
        let honest = self.views(&public);
        let Some(run) = &mut self.runs.run else {
            return;
        };
        let sides = run.sides.lock().map(|sides| sides.clone()).unwrap_or_default();
        let total: u64 = all.iter().chain(&honest).filter_map(|view| view.status.as_ref()).map(|status| status.hashrate).sum();
        let mut picture = match kind {
            LabKind::Attack => {
                let mut victim_side: Vec<&LocalView> = honest.iter().collect();
                victim_side.extend(all.get(1));
                // The attacker, then a pool's miners.
                let attacker_side: Vec<&LocalView> = all.first().into_iter().chain(all.iter().skip(2)).collect();
                Picture { upper: sides.row(all.get(1), &victim_side, total), lower: sides.row(all.first(), &attacker_side, total), replaced: Vec::new() }
            }
            LabKind::Split => {
                let left: Vec<&LocalView> = all.iter().take(all.len().saturating_sub(1)).collect();
                let right: Vec<&LocalView> = all.last().into_iter().collect();
                Picture { upper: sides.row(all.first(), &left, total), lower: sides.row(all.last(), &right, total), replaced: Vec::new() }
            }
        };
        if let Some(before) = run.before.lock().ok().and_then(|before| before.clone()) {
            let mut lost = replaced_between(&before.upper, &picture.upper);
            lost.extend(replaced_between(&before.lower, &picture.lower));
            lost.sort_by_key(|mark| (mark.height, mark.id.clone()));
            lost.dedup();
            picture.replaced = lost;
        }
        if let Ok(mut live) = run.live.lock() {
            *live = picture.clone();
        }
        let ended = match run.state.lock() {
            Ok(mut state) => {
                state.live = picture;
                let ended = matches!(state.phase, Phase::Done | Phase::Failed);
                self.runs.experiment = Some(state.clone());
                ended
            }
            Err(_) => true,
        };
        if ended {
            if let Some(thread) = run.thread.take() {
                let _ = thread.join();
            }
            self.runs.run = None;
            for member in &mut self.members {
                member.busy = false;
            }
            if kind == LabKind::Attack {
                self.tell_race(ids[0]);
            }
        }
    }

    fn making_turn(&mut self) {
        let Some((maker, from, since)) = self.runs.maker else {
            return;
        };
        let Some((_, count)) = self.runs.making else {
            return;
        };
        let target = from + count;
        let lead = self.member(maker).map_or(0, Member::height);
        self.runs.making = Some((lead.saturating_sub(from).min(count), count));
        let reached = lead >= target;
        let everyone = self.members.iter().all(|member| member.height() >= target);
        if reached && (everyone || since.elapsed() > MAKE_LAG + Duration::from_secs(count / 10)) {
            let paused = self.member(maker).is_some_and(|member| member.paused);
            self.send(maker, "board mine-to 0");
            // Its own limit goes back on the next turn.
            self.told.remove(&(maker, "limit"));
            if paused {
                self.send(maker, "board mine off");
            }
            self.runs.maker = None;
            self.runs.making = None;
        }
    }

    fn tricks_turn(&mut self) {
        if let Ok(mut all) = self.runs.verdicts.lock() {
            all.clear();
            for member in &self.members {
                if let Some(status) = &member.status {
                    all.insert(member.id, status.verdicts.clone());
                }
            }
        }
        let mut answers = Vec::new();
        for (exploiter, inbox) in &self.runs.tricks {
            while let Ok(answer) = inbox.try_recv() {
                answers.push((*exploiter, answer));
            }
        }
        for (exploiter, (target, trick, answer)) in answers {
            let row = match answer {
                Ok(outcome) => {
                    let rule = outcome.reason.clone().unwrap_or_default();
                    if outcome.refused {
                        self.event(What::Refused { from: exploiter, by: target, trick, rule: rule.clone() });
                    }
                    TrickRow { trick, by: target, refused: outcome.refused, expected: outcome.refused_as_expected(), rule, failed: None }
                }
                Err(error) => TrickRow { trick, by: target, refused: false, expected: false, rule: String::new(), failed: Some(error) },
            };
            if let Some(member) = self.member_mut(exploiter) {
                member.tricks.push(row);
            }
        }
    }

    /// The network's own folder, where its coin and nodes live.
    pub(super) fn folder(&self) -> Option<PathBuf> {
        self.coin.as_ref().and_then(|coin| coin.ancestors().nth(3)).map(PathBuf::from)
    }
}
