//! A history row's status is never cut. Home's short list and the Wallet's full list show
//! "100블록 뒤 사용 가능" and every other state whole on a window 1280 wide, at normal text and
//! at 150 % (853 points then), in basic Korean and expert English, and the amounts and the clock
//! whole too; a join's words are never cut inside a number. `OLC_SHOTS` keeps each as a picture.
use super::shots::shot;
use super::{app, harness, quiet_settings, scratch, Window, SETTLE_STEPS, SIZES};
use crate::core::{unix_ms, Activity, ActivityKind, Snapshot};
use crate::i18n::{self, Lang, Mode, Voice};
use crate::pages::history::{blocks_to_ripe, confirmations, every_row, signed_amount, SETTLED};
use crate::ui::format;
use egui::{Color32, FontId, TextStyle};
use egui_kittest::kittest::{By, NodeT, Queryable};
use olc_core::CoinConfig;
use std::sync::Arc;

const TIP: u64 = 500;

fn row(kind: ActivityKind, height: Option<u64>, amount: u64, ago_s: u64, ripe_at: Option<u64>, id: u8) -> Activity {
    let time_ms = unix_ms() - ago_s * 1_000;
    Activity { kind, height, amount, time_ms, id: [id; 32], fee: Some(1_000), size: 400, coins_in: 16, ripe_at }
}

/// Every state a row can be in, with the longest ripening count a coin allows and a large amount.
fn staged() -> Snapshot {
    let mut snap = Snapshot::default();
    snap.node.coin = Some(CoinConfig::olc());
    snap.node.height = TIP;
    snap.node.activity = vec![row(ActivityKind::Sent, None, 25_000_000, 1, None, 1)];
    snap.node.wallet.history = Arc::new(vec![
        row(ActivityKind::Mined, Some(TIP), 5_000_000_000, 4, Some(TIP + 101), 2),
        row(ActivityKind::Mined, Some(TIP - 1), 5_000_000_000, 37, Some(TIP + 3), 3),
        row(ActivityKind::Joined, Some(TIP - 2), 123_456_789_012, 59, None, 4),
        row(ActivityKind::Received, Some(TIP - 3), 98_765_432_101, 3_599, None, 5),
        row(ActivityKind::Sent, Some(TIP - 400), 1_234_567_890, 86_000, None, 6),
    ]);
    snap
}

fn word(window: &Window, key: &str) -> &'static str {
    let settings = window.state().settings();
    i18n::text(Voice { lang: settings.language, mode: settings.mode }, key)
}

/// How wide `text` stands whole in `font`.
fn whole(window: &Window, text: &str, font: FontId) -> f32 {
    window.ctx.fonts_mut(|fonts| fonts.layout_no_wrap(text.to_string(), font, Color32::WHITE).size().x)
}

/// Two layouts of one text differ by under a point; a cut text loses at least an ellipsis.
const ROUNDING: f32 = 2.0;

/// Every label holding exactly `text` is drawn whole: as wide as the text laid out in `font`.
#[track_caller]
fn drawn_whole(window: &Window, text: &str, font: &FontId, place: &str) {
    let rects: Vec<egui::Rect> = window.query_all_by_label(text).map(|node| node.rect()).collect();
    assert!(!rects.is_empty(), "{place} shows {text}");
    let wanted = whole(window, text, font.clone());
    for rect in rects {
        assert!(rect.width() + ROUNDING >= wanted, "{place}: {text} is cut to {} of {wanted}", rect.width());
    }
}

/// A row's time is drawn whole, with how long ago or, where the column is narrow, the clock alone.
#[track_caller]
fn time_whole(window: &Window, clock: &str, font: &FontId, place: &str) {
    let mut drawn = 0;
    // A label keeps its words in the value, as kittest reads them.
    for node in window.query_all(By::new().predicate(|node| node.value().is_some_and(|text| text.starts_with(clock)))) {
        let text = node.accesskit_node().value().unwrap_or_default().to_string();
        let wanted = whole(window, &text, font.clone());
        assert!(node.rect().width() + ROUNDING >= wanted, "{place}: {text} is cut to {} of {wanted}", node.rect().width());
        drawn += 1;
    }
    assert!(drawn > 0, "{place} shows the time {clock}");
}

/// The join's words are whole where they fit. Too narrow, they are the short form with no amounts,
/// whose count stands whole: a cut never falls inside a number. Home's narrow list has the short form.
#[track_caller]
fn join_readable(window: &Window, coins_in: usize, font: &FontId, place: &str, narrow: bool) {
    let count = format::number(u128::try_from(coins_in).unwrap_or(u128::MAX));
    let short = i18n::fill(word(window, "history.joined_short"), &[&count]);
    let head = &short[..short.find(&count).map_or(short.len(), |at| at + count.len())];
    let node = window.query_all(By::new().predicate(|node| node.value().is_some_and(|text| text.starts_with(head)))).next();
    let node = node.unwrap_or_else(|| panic!("{place} shows the join as {head}"));
    let text = node.accesskit_node().value().unwrap_or_default().to_string();
    let width = node.rect().width() + ROUNDING;
    if text == short {
        assert!(width >= whole(window, head, font.clone()), "{place}: the count in {text} is cut to {width}");
    } else {
        assert!(!narrow, "{place}: Home's narrow list says the join short, not {text}");
        assert!(width >= whole(window, &text, font.clone()), "{place}: {text} is cut to {width}");
    }
}

/// The words a row's status says, worked out apart from the page. A waiting row's tag is painted
/// with no label to find, and a tag is never cut.
fn status_said(window: &Window, item: &Activity, tip: u64) -> Option<String> {
    item.height?;
    if let Some(left) = blocks_to_ripe(item, tip) {
        return Some(i18n::fill(word(window, "history.ripe_in"), &[&format::number(left.into())]));
    }
    let count = confirmations(item, tip).unwrap_or(0);
    if window.state().settings().mode == Mode::Expert {
        return Some(i18n::fill(word(window, "history.confirmations"), &[&format::number(count.into())]));
    }
    Some(word(window, if count >= SETTLED { "history.settled" } else { "history.confirming" }).to_string())
}

#[test]
fn the_history_status_stands_whole_at_1280_and_at_150_percent() {
    let folder = std::env::var_os("OLC_SHOTS").map(std::path::PathBuf::from);
    let mut window = harness(app(scratch("history-columns"), quiet_settings(), None), SIZES[0]);
    let stage = staged();
    window.state_mut().set_stage(Some(stage.clone()));
    for (mode, language) in [(Mode::Basic, Lang::Ko), (Mode::Expert, Lang::En)] {
        for percent in [100_u16, 150] {
            let settings = window.state_mut().settings_mut();
            settings.mode = mode;
            settings.language = language;
            settings.text_percent = percent;
            // The zoom lands first, then the window is 1280 by 800 pixels in points of that zoom.
            window.run_steps(2);
            let scale = f32::from(percent) / 100.0;
            window.set_size(egui::vec2(SIZES[0].0 as f32 / scale, SIZES[0].1 as f32 / scale));
            let body = window.ctx.global_style().text_styles[&TextStyle::Body].clone();
            let medium = FontId::new(body.size, crate::ui::fonts::medium());
            for page in ["page.home", "page.wallet"] {
                window.state_mut().open(page);
                window.run_steps(SETTLE_STEPS);
                let place = format!("{page} {mode:?} {language:?} at {percent} %");
                // Home holds the newest five rows, so the oldest is the Wallet's alone.
                let rows: Vec<&Activity> = every_row(&stage.node).collect();
                let shown = if page == "page.home" { &rows[..5] } else { &rows[..] };
                for item in shown {
                    if let Some(status) = status_said(&window, item, stage.node.height) {
                        drawn_whole(&window, &status, &body, &place);
                    }
                    drawn_whole(&window, &signed_amount(item, "OLC"), &medium, &place);
                    time_whole(&window, &format::clock(unix_ms(), item.time_ms), &body, &place);
                    if item.kind == ActivityKind::Joined {
                        join_readable(&window, item.coins_in, &body, &place, page == "page.home");
                    }
                }
                if let Some(folder) = &folder {
                    let name = format!("history-columns-{}-{mode:?}-{language:?}-{percent}", page.trim_start_matches("page."));
                    shot(&mut window, folder, &name.to_lowercase());
                }
            }
        }
    }
    window.state_mut().set_stage(None);
}
