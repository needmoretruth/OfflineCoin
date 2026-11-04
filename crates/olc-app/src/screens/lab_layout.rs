//! The map keeps every participant apart as it fills: no token with its words lies on another's,
//! and all of them stay on the map, at the window sizes a person uses. Real processes, added by
//! clicks: `cargo test -p olc-app --lib lab_layout -- --ignored`.
use super::lab_net::LabRun;
use super::SETTLE_STEPS;
use crate::i18n::{Lang, Mode, Voice};
use crate::local::lab_net::Kind;
use crate::pages::lab_tokens::kind_key;
use egui::accesskit::Role;
use egui::Rect;
use egui_kittest::kittest::{By, NodeT, Queryable};

const SIZES: [(u32, u32); 3] = [(960, 620), (1280, 800), (1920, 1080)];
/// The order a person might add them in: every kind early, then round again.
const ORDER: [Kind; 12] = [
    Kind::Miner,
    Kind::Pool,
    Kind::Miner,
    Kind::Full,
    Kind::Sender,
    Kind::Receiver,
    Kind::Miner,
    Kind::Pruned,
    Kind::Light,
    Kind::Spammer,
    Kind::Attacker,
    Kind::Exploiter,
];
/// Counts on the map at which it is measured; the last is as many as the room line allows.
const COUNTS: [usize; 3] = [3, 9, 15];

/// Each token on the map with its words, by the name it is read by.
fn tokens(run: &LabRun) -> Vec<(String, Rect)> {
    let mut words: Vec<&str> = Kind::ALL.iter().map(|kind| run.say(kind_key(*kind))).collect();
    // "Bad miner 1" must not be read as "Miner 1" of a longer name.
    words.sort_by_key(|word| std::cmp::Reverse(word.len()));
    let is_token = |label: &str| {
        words.iter().any(|word| {
            let Some(rest) = label.strip_prefix(word).and_then(|rest| rest.strip_prefix(' ')) else {
                return false;
            };
            let number_end = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
            number_end > 0 && (rest.len() == number_end || rest[number_end..].starts_with(", "))
        })
    };
    run.window
        .query_all(By::new().role(Role::Button))
        .filter_map(|node| {
            let label = node.accesskit_node().label()?;
            is_token(&label).then(|| (label.split(", ").next().unwrap_or(&label).to_string(), node.rect()))
        })
        .collect()
}

/// What overlaps or leaves the map at `size`, with `count` on it.
fn wrong_at(run: &mut LabRun, size: (u32, u32), count: usize) -> Vec<String> {
    run.window.set_size(egui::vec2(size.0 as f32, size.1 as f32));
    run.window.run_steps(SETTLE_STEPS);
    let at = format!("{}x{} with {count}", size.0, size.1);
    let map_label = run.say("net.map");
    let Some(map) = run.window.query(By::new().label(map_label)).map(|node| node.rect()) else {
        return vec![format!("{at}: no map")];
    };
    let found = tokens(run);
    let mut wrong = Vec::new();
    if found.len() != count {
        wrong.push(format!("{at}: {} tokens on the map", found.len()));
    }
    let pool = run.say(kind_key(Kind::Pool));
    let is_pool = |name: &str| name.strip_prefix(pool).is_some_and(|rest| rest.starts_with(' '));
    for (index, (name, rect)) in found.iter().enumerate() {
        if !map.expand(0.5).contains_rect(*rect) {
            wrong.push(format!("{at}: {name} {rect:?} leaves the map {map:?}"));
        }
        for (other, other_rect) in &found[index + 1..] {
            let inside = (is_pool(name) && rect.contains_rect(*other_rect)) || (is_pool(other) && other_rect.contains_rect(*rect));
            if !inside && rect.shrink(0.5).intersects(other_rect.shrink(0.5)) {
                wrong.push(format!("{at}: {name} {rect:?} lies on {other} {other_rect:?}"));
            }
        }
    }
    wrong
}

fn room_left(run: &LabRun) -> usize {
    run.line_numbers("net.room").and_then(|numbers| numbers.first().copied()).unwrap_or(0.0) as usize
}

/// Three, nine, fifteen and as many as the room line allows: no two tokens with their words
/// overlap and every one stays on the map, at 960 by 620, 1280 by 800 and 1920 by 1080.
#[test]
#[ignore]
fn no_two_participants_overlap_as_the_map_fills() {
    let voice = Voice { lang: Lang::Ko, mode: Mode::Basic };
    let mut run = LabRun::start("lab-layout", voice);
    let most = room_left(&run);
    assert!(most >= 3, "room for {most} only");
    let mut added = 0;
    let mut joined = false;
    let mut wrong = Vec::new();
    for target in COUNTS.into_iter().filter(|count| *count < most).chain([most]) {
        while added < target && room_left(&run) > 0 {
            run.add(ORDER[added % ORDER.len()]);
            added += 1;
        }
        if !joined && added >= 9 {
            // Miners inside a pool are part of a full map.
            let pool = run.named(Kind::Pool, 1);
            for number in [1, 2] {
                let miner = run.named(Kind::Miner, number);
                run.drag(&miner, &pool);
            }
            joined = true;
        }
        run.wait(90, "every token on the map", |run| tokens(run).len() == added);
        for size in SIZES {
            let found = wrong_at(&mut run, size, added);
            println!("{}x{} with {added}: {} wrong", size.0, size.1, found.len());
            wrong.extend(found);
        }
        run.back_to_normal();
        let map_label = run.say("net.map");
        run.window.get_by_label(map_label).scroll_to_me();
        run.window.run_steps(SETTLE_STEPS);
        run.shoot(&format!("map-{added}"));
    }
    let clear = run.say("net.clear");
    run.press(clear);
    run.none_left("clearing the map");
    run.close();
    assert!(wrong.is_empty(), "{} overlaps or tokens off the map:\n{}", wrong.len(), wrong.join("\n"));
}
