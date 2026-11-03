//! The Lab on a real run: miners on this computer race, and the Lab's own drawing shows each
//! step. With `OLC_SHOTS` naming a folder it keeps a picture of each step at 1280 by 800, in
//! plain Korean, and with "Details" open at the end in plain Korean and expert English. The page
//! itself is checked in the app too: expert mode reaches it from Boards.
use super::{app, harness, node_program, quiet_settings, scratch, SETTLE_STEPS};
use crate::i18n::{self, Lang, Mode, Voice};
use crate::local::lab::{use_place, LabChoice, LabKind, LabRun, LabState, Phase};
use crate::pages::lab_view::{self, Seen};
use crate::ui::theme::apply;
use crate::ui::tokens::{PAGE_MARGIN, DARK};
use crate::ui::{fonts, shell};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use olc_lab::{Outcome, Step};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

struct Shown {
    state: Option<LabState>,
    choice: LabChoice,
    voice: Voice,
    fonts: bool,
}

fn draw(ui: &mut egui::Ui, shown: &mut Shown) {
    let ctx = ui.ctx().clone();
    if !shown.fonts {
        shown.fonts = true;
        ctx.set_fonts(fonts::definitions());
        return;
    }
    apply(&ctx, &DARK);
    shell::paint_back(ui, &DARK);
    egui::Frame::new().inner_margin(egui::Margin::same(PAGE_MARGIN)).show(ui, |ui| {
        let running = shown.state.as_ref().is_some_and(|state| !matches!(state.phase, Phase::Done | Phase::Failed));
        let seen = Seen { state: shown.state.as_ref(), running, kept: false };
        lab_view::draw(ui, &DARK, shown.voice, &mut shown.choice, &seen);
    });
}

type Window = Harness<'static, Shown>;

fn lab_window(size: (f32, f32), voice: Voice, choice: &LabChoice) -> Window {
    let shown = Shown { state: None, choice: choice.clone(), voice, fonts: false };
    let mut window = Harness::builder().with_size(egui::vec2(size.0, size.1)).with_render_cursor(false).build_ui_state(draw, shown);
    window.run_steps(SETTLE_STEPS);
    window
}

fn shoot(window: &mut Window, folder: Option<&Path>, name: &str) {
    window.run_steps(SETTLE_STEPS);
    if let Some(folder) = folder {
        let image = window.render().expect("a renderer");
        std::fs::create_dir_all(folder).expect("the picture folder");
        image.save(folder.join(format!("{name}.png"))).expect("save");
    }
}

fn place(name: &str) {
    let folder: PathBuf = scratch(name);
    use_place(node_program(), folder.join("miners"));
}

/// The Lab has its own place on the rail in both modes, and its run starts from the first screen.
#[test]
fn the_lab_page_offers_the_run_on_this_computer() {
    let mut window = harness(app(scratch("lab_page"), quiet_settings(), None), (1280, 800));
    for mode in [Mode::Basic, Mode::Expert] {
        window.state_mut().settings_mut().mode = mode;
        window.state_mut().open("page.lab");
        window.run_steps(SETTLE_STEPS);
        let start = i18n::text(Voice { lang: Lang::En, mode }, "lab2.start_attack");
        assert!(window.query_by_label(start).is_some(), "no start button on the Lab in {mode:?}");
    }
    if let Some(folder) = std::env::var_os("OLC_SHOTS").map(PathBuf::from) {
        window.state_mut().settings_mut().language = Lang::Ko;
        super::shots::shot(&mut window, &folder, "lab-page-expert-ko-1280");
    }
}

/// Runs `choice` on real miners and, with `OLC_SHOTS`, keeps a picture of each step as the page
/// showed it then, and of Details at the end in plain Korean and expert English.
fn run_and_shoot(choice: LabChoice, prefix: &str) -> LabState {
    place(prefix);
    let folder = std::env::var_os("OLC_SHOTS").map(PathBuf::from);
    let basic = Voice { lang: Lang::Ko, mode: Mode::Basic };
    let mut window = lab_window((1280.0, 800.0), basic, &choice);
    shoot(&mut window, folder.as_deref(), &format!("{prefix}-00-idle-basic-ko-1280"));
    let run = LabRun::start(choice.clone());
    let until = Instant::now() + Duration::from_secs(600);
    let mut drawn = 0;
    let state = loop {
        let state = run.state();
        if state.frames.len() > drawn {
            drawn = state.frames.len();
            println!("step {drawn}: {:?}", state.frames.last().map(|frame| &frame.step));
        }
        window.state_mut().state = Some(state.clone());
        window.run_steps(1);
        if !run.running() {
            // The run may have ended after this state was read.
            break run.state();
        }
        assert!(Instant::now() < until, "the run took too long");
        std::thread::sleep(Duration::from_millis(300));
    };
    assert_eq!(state.phase, Phase::Done, "{:?}", state.error);
    for (index, frame) in state.frames.iter().enumerate() {
        let mut then = state.clone();
        then.frames.truncate(index + 1);
        then.live = frame.picture.clone();
        let last = index + 1 == state.frames.len();
        then.phase = if last { Phase::Done } else { Phase::Running };
        window.state_mut().state = Some(then);
        let step = format!("{:?}", frame.step);
        let word: String = step.chars().take_while(char::is_ascii_alphabetic).collect();
        shoot(&mut window, folder.as_deref(), &format!("{prefix}-{:02}-{}-basic-ko-1280", index + 1, word.to_lowercase()));
    }
    for (voice, name) in [(basic, "basic-ko"), (Voice { lang: Lang::En, mode: Mode::Expert }, "expert-en")] {
        let mut open = lab_window((1280.0, 2300.0), voice, &choice);
        open.state_mut().state = Some(state.clone());
        open.run_steps(SETTLE_STEPS);
        open.get_by_label(i18n::text(voice, "explain.details")).click();
        shoot(&mut open, folder.as_deref(), &format!("{prefix}-details-{name}-1280"));
    }
    state
}

#[test]
#[ignore = "mines for a minute on real node processes: run it by name"]
fn a_real_attack_is_drawn_step_by_step() {
    let state = run_and_shoot(LabChoice::default(), "lab-attack");
    let last = state.frames.last().map(|frame| &frame.step);
    match state.outcome {
        Some(Outcome::Gone) => assert_eq!(last, Some(&Step::Gone)),
        Some(Outcome::Stays) => assert_eq!(last, Some(&Step::Stays)),
        None => panic!("no outcome"),
    }
}

#[test]
#[ignore = "mines for a minute on real node processes: run it by name"]
fn a_real_split_is_drawn_step_by_step() {
    let state = run_and_shoot(LabChoice { kind: LabKind::Split, ..LabChoice::default() }, "lab-split");
    let settled = state.frames.iter().any(|frame| matches!(frame.step, Step::Settled { .. }));
    assert!(settled, "{:?}", state.frames.last());
}
