//! The finer rows of a board, for an expert. Each sends one word the board already knows.
use super::board_detail::{tell, BoardDetail};
use crate::core::{BoardAction, BoardView};
use super::Cx;
use crate::ui::format;
use crate::ui::register::{group, row};
use crate::ui::widgets::{button, button_on, field, meta, mono, number_field, Kind};
use egui::{Label, Ui};
use olc_control::BoardId;
use std::collections::HashMap;

/// Clocks the board's chip can run at, in MHz.
const CLOCKS: [u32; 3] = [80, 160, 240];
/// A board keeps few keys, and these are the ones a person picks by number.
const KEYS_SHOWN: usize = 3;
/// Speeds the board's wire link is tried at.
const WIRE_SPEEDS: [u32; 3] = [115_200, 460_800, 921_600];

fn cap_rows(detail: &mut BoardDetail, ui: &mut Ui, cx: &Cx, board: &BoardView) {
    let t = cx.t;
    let id = board.seen.id;
    let beacon = &board.seen.beacon;
    let about = match beacon.limit {
        0 => cx.say("boards.no_cap").to_string(),
        limit => format::rate(cx.voice(), limit.into()),
    };
    row(ui, t, cx.say("pro.cap"), &about, |ui| {
        number_field(ui, &mut detail.cap, cx.say("pro.cap"), 90.0);
        if button(ui, t, cx.say("common.apply"), Kind::Plain).clicked() {
            tell(cx, id, BoardAction::Cap(detail.cap));
        }
        if button(ui, t, cx.say("pro.default_cap"), Kind::Quiet).clicked() {
            tell(cx, id, BoardAction::DefaultCap);
        }
    });
    row(ui, t, cx.say("pro.full_speed"), cx.say("pro.full_speed_about"), |ui| {
        let full = beacon.limit == 0;
        let key = if full { "pro.full_off" } else { "pro.full_on" };
        if button(ui, t, cx.say(key), Kind::Plain).clicked() {
            tell(cx, id, BoardAction::FullSpeed(!full));
        }
    });
    row(ui, t, cx.say("pro.clock"), cx.say("pro.clock_about"), |ui| {
        for clock in CLOCKS {
            if button(ui, t, &format!("{clock} MHz"), Kind::Plain).clicked() {
                tell(cx, id, BoardAction::Clock(clock));
            }
        }
    });
    let (lowest, highest) = beacon.smooth_range;
    let range = cx.fill("pro.smooth_about", &[&lowest.to_string(), &highest.to_string()]);
    row(ui, t, cx.say("pro.smooth"), &range, |ui| {
        number_field(ui, &mut detail.smooth, cx.say("pro.smooth"), 64.0);
        if button(ui, t, cx.say("common.apply"), Kind::Plain).clicked() {
            detail.smooth = detail.smooth.clamp(u64::from(lowest), u64::from(highest));
            tell(cx, id, BoardAction::Smooth(detail.smooth));
        }
        if button(ui, t, cx.say("pro.show_logo"), Kind::Quiet).clicked() {
            tell(cx, id, BoardAction::ShowLogo);
        }
    });
}

/// A row with one text box and the word it sends.
/// A row with its own text box. Each row keeps its own words in `words`, under its key.
fn word_row(
    ui: &mut Ui,
    cx: &Cx,
    (name, about): (&'static str, &'static str),
    words: &mut HashMap<&'static str, String>,
    send: &mut dyn FnMut(String),
) {
    let t = cx.t;
    let text = words.entry(name).or_default();
    row(ui, t, cx.say(name), cx.say(about), |ui| {
        field(ui, text, cx.say(name), 190.0);
        let ready = !text.trim().is_empty();
        if button_on(ui, t, cx.say("common.apply"), Kind::Plain, ready).clicked() {
            send(std::mem::take(text).trim().to_string());
        }
    });
}

fn radio_rows(detail: &mut BoardDetail, ui: &mut Ui, cx: &Cx, id: BoardId) {
    let t = cx.t;
    row(ui, t, cx.say("pro.radio_power"), cx.say("pro.radio_power_about"), |ui| {
        for (dbm, key) in [(2, "pro.power_low"), (8, "pro.power_mid"), (20, "pro.power_full")] {
            if button(ui, t, cx.say(key), Kind::Plain).clicked() {
                tell(cx, id, BoardAction::RadioPower(dbm));
            }
        }
    });
    let words = &mut detail.words;
    word_row(ui, cx, ("pro.dial", "pro.dial_about"), words, &mut |text| {
        tell(cx, id, BoardAction::Dial(text));
    });
    row(ui, t, cx.say("pro.dial_off"), cx.say("pro.dial_off_about"), |ui| {
        if button(ui, t, cx.say("pro.dial_off"), Kind::Plain).clicked() {
            tell(cx, id, BoardAction::DialOff);
        }
    });
    word_row(ui, cx, ("pro.links", "pro.links_about"), words, &mut |text| {
        tell(cx, id, BoardAction::Links(text));
    });
    word_row(ui, cx, ("pro.ignore", "pro.ignore_about"), words, &mut |text| {
        tell(cx, id, BoardAction::Ignore(text));
    });
    row(ui, t, cx.say("pro.wires"), cx.say("pro.wires_about"), |ui| {
        if button(ui, t, cx.say("pro.wires_swap"), Kind::Plain).clicked() {
            tell(cx, id, BoardAction::SwapWires);
        }
        for baud in WIRE_SPEEDS {
            if button(ui, t, &format::number(baud.into()), Kind::Quiet).clicked() {
                tell(cx, id, BoardAction::WireSpeed(baud));
            }
        }
    });
    row(ui, t, cx.say("pro.hear_all"), cx.say("pro.hear_all_about"), |ui| {
        if button(ui, t, cx.say("pro.hear_all"), Kind::Plain).clicked() {
            tell(cx, id, BoardAction::HearEveryone);
        }
    });
    word_row(ui, cx, ("pro.trust", "pro.trust_about"), words, &mut |text| {
        tell(cx, id, BoardAction::Trust(text));
    });
}

fn chain_rows(detail: &mut BoardDetail, ui: &mut Ui, cx: &Cx, id: BoardId) {
    let t = cx.t;
    let words = &mut detail.words;
    word_row(ui, cx, ("pro.mine_to", "pro.mine_to_about"), words, &mut |text| {
        tell(cx, id, BoardAction::MineTo(text));
    });
    row(ui, t, cx.say("pro.keys"), cx.say("pro.keys_about"), |ui| {
        if button(ui, t, cx.say("pro.new_key"), Kind::Plain).clicked() {
            tell(cx, id, BoardAction::NewKey);
        }
        if button(ui, t, cx.say("pro.own_key"), Kind::Plain).clicked() {
            tell(cx, id, BoardAction::MineToOwnKey);
        }
        for index in 0..KEYS_SHOWN {
            if button(ui, t, &format!("{index}"), Kind::Quiet).clicked() {
                tell(cx, id, BoardAction::UseKey(index));
            }
        }
    });
    row(ui, t, cx.say("pro.export_key"), cx.say("pro.export_key_about"), |ui| {
        for index in 0..KEYS_SHOWN {
            if button(ui, t, &format!("{index}"), Kind::Quiet).clicked() {
                tell(cx, id, BoardAction::ExportKey(index));
            }
        }
    });
    word_row(ui, cx, ("pro.import_key", "pro.import_key_about"), words, &mut |text| {
        tell(cx, id, BoardAction::ImportKey(text));
    });
    row(ui, t, cx.say("pro.keep"), cx.say("pro.keep_about"), |ui| {
        if button(ui, t, cx.say("pro.keep_all"), Kind::Plain).clicked() {
            tell(cx, id, BoardAction::Keep(None));
        }
        for blocks in [1_000, 10_000] {
            if button(ui, t, &format::number(blocks), Kind::Plain).clicked() {
                tell(cx, id, BoardAction::Keep(Some(blocks as u64)));
            }
        }
    });
    for (name, about, make) in [
        ("pro.merge", "pro.merge_about", BoardAction::Merge as fn(bool) -> BoardAction),
        ("pro.light", "pro.light_about", BoardAction::Light),
    ] {
        row(ui, t, cx.say(name), cx.say(about), |ui| {
            for (key, on) in [("common.on", true), ("common.off", false)] {
                if button(ui, t, cx.say(key), Kind::Plain).clicked() {
                    tell(cx, id, make(on));
                }
            }
        });
    }
    word_row(ui, cx, ("pro.note", "pro.note_about"), words, &mut |text| {
        tell(cx, id, BoardAction::Note(text));
    });
    word_row(ui, cx, ("pro.mine_note", "pro.mine_note_about"), words, &mut |text| {
        tell(cx, id, BoardAction::MineNote(text));
    });
    row(ui, t, cx.say("pro.mine_note_off"), cx.say("pro.mine_note_off_about"), |ui| {
        if button(ui, t, cx.say("pro.mine_note_off"), Kind::Plain).clicked() {
            tell(cx, id, BoardAction::MineNoteOff);
        }
    });
    row(ui, t, cx.say("pro.snapshot"), cx.say("pro.snapshot_about"), |ui| {
        if button(ui, t, cx.say("history.accept"), Kind::Plain).clicked() {
            tell(cx, id, BoardAction::AcceptSnapshot);
        }
    });
    row(ui, t, cx.say("pro.coin"), cx.say("pro.coin_about"), |ui| {
        if button(ui, t, cx.say("pro.default_coin"), Kind::Plain).clicked() {
            tell(cx, id, BoardAction::DefaultCoin);
        }
        if button(ui, t, cx.say("pro.confirm_coin"), Kind::Plain).clicked() {
            tell(cx, id, BoardAction::ConfirmCoin);
        }
    });
    row(ui, t, cx.say("pro.clear_payments"), cx.say("pro.clear_payments_about"), |ui| {
        if button(ui, t, cx.say("pro.clear_payments"), Kind::Plain).clicked() {
            tell(cx, id, BoardAction::ClearWaitingPayments);
        }
    });
    row(ui, t, cx.say("pro.stop_work"), cx.say("pro.stop_work_about"), |ui| {
        if button(ui, t, cx.say("pro.stop_work"), Kind::Plain).clicked() {
            tell(cx, id, BoardAction::StopWork);
        }
    });
}

fn read_rows(detail: &mut BoardDetail, ui: &mut Ui, cx: &Cx, board: &BoardView) {
    let t = cx.t;
    let id = board.seen.id;
    let reads = [
        ("pro.read_status", BoardAction::Status),
        ("pro.read_starts", BoardAction::RecentStarts),
        ("pro.read_screen", BoardAction::ReadScreen),
        ("pro.read_keys", BoardAction::ReadKeys),
        ("pro.read_coin", BoardAction::ReadCoin),
        ("pro.read_shares", BoardAction::ReadShares),
        ("pro.read_near", BoardAction::Neighbours),
        ("pro.read_wires", BoardAction::ReadWires),
    ];
    row(ui, t, cx.say("pro.read"), cx.say("pro.read_about"), |ui| {
        for (key, action) in reads {
            if button(ui, t, cx.say(key), Kind::Quiet).clicked() {
                tell(cx, id, action);
            }
        }
    });
    let words = &mut detail.words;
    word_row(ui, cx, ("pro.read_tx", "pro.read_tx_about"), words, &mut |text| {
        tell(cx, id, BoardAction::ReadPayment(text));
    });
    if let Some(answer) = &board.answer {
        ui.add(Label::new(mono(answer.clone())).wrap());
    } else {
        meta(ui, cx.say("pro.read_note"), t);
    }
}

pub fn expert_rows(detail: &mut BoardDetail, ui: &mut Ui, cx: &Cx, board: &BoardView) {
    let id = board.seen.id;
    group(ui, cx.say("pro.board_mining"));
    cap_rows(detail, ui, cx, board);
    group(ui, cx.say("pro.board_radio"));
    radio_rows(detail, ui, cx, id);
    group(ui, cx.say("pro.board_chain"));
    chain_rows(detail, ui, cx, id);
    group(ui, cx.say("pro.board_read"));
    read_rows(detail, ui, cx, board);
}
