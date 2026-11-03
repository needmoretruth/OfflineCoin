//! A quiet button has no edge, so its text is where it seems to stand. Measured on every page in
//! both modes: the text of a quiet button that starts or ends a column lines up with something else
//! in that column, as a heading, a line of text or a table's heading does.
use super::shots::with_boards;
use super::{app, harness, quiet_settings, scratch, wait_until, Window, SETTLE_STEPS, SIZES};
use crate::i18n::{self, Lang, Mode, Voice};
use egui::accesskit::Role;
use egui::Rect;
use egui_kittest::kittest::{By, NodeT, Queryable};

/// The room a button keeps on each side of its text, as `widgets` draws it.
const PAD: f32 = 14.0;
/// How far apart two edges may be and still read as one line.
const SAME: f32 = 1.5;
/// How far above or below a quiet button its column is looked for.
const NEAR: f32 = 360.0;
/// The quiet buttons that start or end a column: Details and Hide under an explanation, the miners'
/// Remove, Home's See all, and the way back from locks. The blocks table's Open says what the
/// Wallet's plain Open buttons say, so the walk cannot tell it apart by its words.
const QUIET: [&str; 6] = ["explain.details", "explain.hide", "mining.more", "home.see_all", "local.remove", "wallet_page.back"];

fn word(window: &Window, key: &str) -> String {
    let settings = window.state().settings();
    i18n::text(Voice { lang: settings.language, mode: settings.mode }, key).to_string()
}

/// Every quiet button on the page whose text lines up with nothing near it.
fn adrift(window: &Window, page: &str, wrong: &mut Vec<String>) -> usize {
    let quiet: Vec<String> = QUIET.iter().map(|key| word(window, key)).collect();
    let all: Vec<(Rect, Role, String)> = window
        .query_all(By::new().predicate(|node| node.bounding_box().is_some()))
        .map(|node| (node.rect(), node.accesskit_node().role(), node.accesskit_node().label().unwrap_or_default()))
        .collect();
    let mut seen = 0;
    for (button, _, label) in all.iter().filter(|(_, role, label)| *role == Role::Button && quiet.contains(label)) {
        seen += 1;
        let (left, right) = (button.left() + PAD, button.right() - PAD);
        let near = |other: &Rect| other != button && (other.center().y - button.center().y).abs() < NEAR && other.width() > 1.0;
        let lined = all.iter().any(|(other, role, _)| {
            *role != Role::Button && near(other) && ((other.left() - left).abs() < SAME || (other.right() - right).abs() < SAME)
        });
        if !lined {
            let lefts: Vec<String> = all
                .iter()
                .filter(|(other, role, _)| *role != Role::Button && near(other) && (other.left() - left).abs() < 30.0)
                .map(|(other, _, name)| format!("{:.1} {}", other.left(), name.chars().take(24).collect::<String>()))
                .collect();
            wrong.push(format!("{page} \"{label}\": text at {left:.1}..{right:.1}, near: {lefts:?}"));
        }
    }
    seen
}

#[test]
fn a_quiet_buttons_text_lines_up_with_its_column() {
    let mut window = harness(app(scratch("quiet-align"), quiet_settings(), None), SIZES[0]);
    wait_until(&mut window, |app| !app.snapshot().node.address.is_empty());
    let quiet = window.state().snapshot();
    let stage = with_boards(&quiet).unwrap_or_else(|| (*quiet).clone());
    window.state_mut().set_stage(Some(stage));
    let mut wrong = Vec::new();
    let mut seen = 0;
    for mode in [Mode::Basic, Mode::Expert] {
        window.state_mut().settings_mut().mode = mode;
        window.state_mut().settings_mut().language = Lang::En;
        for title in window.state().page_titles() {
            window.state_mut().open(title);
            window.run_steps(SETTLE_STEPS);
            seen += adrift(&window, &format!("{title} {mode:?}"), &mut wrong);
        }
    }
    assert!(seen >= 10, "the walk found {seen} quiet buttons");
    assert!(wrong.is_empty(), "quiet buttons whose text stands off their column:\n{}", wrong.join("\n"));
}

/// Nothing a page draws runs past the right edge of the window, in either language or mode: a
/// line wider than its column widens it and pushes the column beside it out of sight.
#[test]
fn nothing_runs_off_the_right_edge() {
    let mut window = harness(app(scratch("right-edge"), quiet_settings(), None), SIZES[0]);
    wait_until(&mut window, |app| !app.snapshot().node.address.is_empty());
    let quiet = window.state().snapshot();
    let stage = with_boards(&quiet).unwrap_or_else(|| (*quiet).clone());
    window.state_mut().set_stage(Some(stage));
    let edge = f64::from(window.ctx.content_rect().right()) + 0.5;
    let mut wrong = Vec::new();
    for language in [Lang::En, Lang::Ko] {
        for mode in [Mode::Basic, Mode::Expert] {
            window.state_mut().settings_mut().mode = mode;
            window.state_mut().settings_mut().language = language;
            for title in window.state().page_titles() {
                window.state_mut().open(title);
                window.run_steps(SETTLE_STEPS);
                let past = window.query_all(By::new().predicate(move |node| node.bounding_box().is_some_and(|rect| rect.x1 > edge)));
                for node in past {
                    let label: String = node.accesskit_node().label().unwrap_or_default().chars().take(30).collect();
                    wrong.push(format!("{title} {language:?} {mode:?}: \"{label}\" ends at {:.0}", node.rect().right()));
                }
            }
        }
    }
    assert!(wrong.is_empty(), "past the right edge of the window:\n{}", wrong.join("\n"));
}
