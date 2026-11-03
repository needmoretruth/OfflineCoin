//! The swap page as a person reads it: each block named with its coin and about when it comes,
//! the chain followed beside this one in plain words, and swaps that ended folded away.
use super::{app, harness, quiet_settings, scratch, SETTLE_STEPS, SIZES};
use crate::core::swap::{Outcome, Stage};
use crate::core::{Snapshot, SwapView, SwapsView};
use crate::i18n::{Lang, Mode, Voice};
use crate::settings::Settings;
use crate::ui::format;
use egui_kittest::kittest::Queryable;
use olc_core::coin::COIN;

fn swap(id: &str, stage: Stage, (give, get): (u64, u64)) -> SwapView {
    SwapView {
        id: id.into(),
        maker: true,
        stage,
        give: ("SWT".into(), give * COIN),
        get: ("TWO".into(), get * COIN),
        text: None,
        refund_at: Some(130),
        height: 10,
        // 120 blocks of 55 seconds each: an hour and fifty minutes.
        block_ms: 55_000,
        wait_minutes: 60,
    }
}

fn staged_swaps() -> Snapshot {
    let swaps = vec![
        swap("aa11bb22", Stage::Locked, (100, 20)),
        swap("cc33dd44", Stage::Ended(Outcome::Swapped), (7, 3)),
        swap("ee55ff66", Stage::Ended(Outcome::Refunded), (9, 4)),
    ];
    Snapshot { swaps: SwapsView { swaps, other: Some(("TWO".into(), 10, 1)), coins: Vec::new() }, ..Snapshot::default() }
}

#[test]
fn a_swap_card_names_each_block_with_its_coin_and_ended_swaps_fold_away() {
    let settings = Settings { language: Lang::Ko, ..quiet_settings() };
    let mut window = harness(app(scratch("swap-page"), settings, None), SIZES[0]);
    window.state_mut().set_stage(Some(staged_swaps()));
    window.state_mut().open("page.swap");
    window.run_steps(SETTLE_STEPS);
    let shows = |window: &super::Window, text: &str| window.query_all_by_label_contains(text).next().is_some();
    assert!(shows(&window, "TWO 블록도 받고 있습니다. 지금 TWO 10번 블록입니다."));
    assert!(shows(&window, "돌아오는 때는 SWT 130번 블록, 약 1시간 50분 뒤입니다. 지금은 SWT 10번 블록입니다."));
    assert!(shows(&window, "100 SWT 주고 20 TWO 받기"));
    assert!(!shows(&window, "7 SWT 주고 3 TWO 받기"), "a finished swap waits under the fold");
    assert!(!shows(&window, "내 코인을 돌려받았습니다."));
    window.query_by_label("끝난 교환 2개 보기").expect("the fold").click();
    window.run_steps(SETTLE_STEPS);
    assert!(shows(&window, "7 SWT 주고 3 TWO 받기"));
    assert!(shows(&window, "내 코인을 돌려받았습니다."));
    window.query_by_label("끝난 교환 숨기기").expect("the fold, open").click();
    window.run_steps(SETTLE_STEPS);
    assert!(!shows(&window, "7 SWT 주고 3 TWO 받기"));
}

#[test]
fn a_wait_reads_in_minutes_hours_or_days() {
    let korean = Voice { lang: Lang::Ko, mode: Mode::Basic };
    assert_eq!(format::span(korean, 20), "1분");
    assert_eq!(format::span(korean, 1_200), "20분");
    assert_eq!(format::span(korean, 3_600), "1시간");
    assert_eq!(format::span(korean, 6_600), "1시간 50분");
    assert_eq!(format::span(korean, 90_000), "1일 1시간");
    assert_eq!(format::span(Voice { lang: Lang::En, mode: Mode::Basic }, 6_600), "1 h 50 min");
}
