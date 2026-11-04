//! Helpers for the Lab's screen tests on real node processes: a window on the Lab with its own
//! place for the network, presses by the words on screen, reading a token's line, dragging a token,
//! and finding the node processes a network left behind.
use super::shots::shot;
use super::{app, harness, node_program, quiet_settings, scratch, Window, SETTLE_STEPS};
use crate::i18n::{self, Lang, Mode, Voice};
use crate::local::lab::use_place;
use crate::local::lab_net::Kind;
use crate::pages::lab_tokens::kind_key;
use crate::settings::Theme;
use egui::accesskit::Role;
use egui_kittest::kittest::{By, NodeT, Queryable};
use std::path::PathBuf;
use std::time::{Duration, Instant};

pub(super) const SHOT_SIZE: (u32, u32) = (1280, 800);

pub(super) struct LabRun {
    pub window: Window,
    pub voice: Voice,
    folder: Option<PathBuf>,
    tag: &'static str,
    /// The scratch folder's name, which every node process of this run has in its command line.
    name: String,
    /// Presses and drags so far, for the count of clicks each result took.
    pub clicks: u32,
}

impl LabRun {
    pub fn start(name: &str, voice: Voice) -> Self {
        let root = scratch(name);
        use_place(node_program(), root.join("lab"));
        let mut window = harness(app(root, quiet_settings(), None), SHOT_SIZE);
        let settings = window.state_mut().settings_mut();
        settings.language = voice.lang;
        settings.mode = voice.mode;
        settings.theme = Theme::Dark;
        settings.text_percent = 100;
        window.state_mut().open("page.lab");
        window.run_steps(SETTLE_STEPS);
        let folder = std::env::var_os("OLC_SHOTS").map(PathBuf::from);
        let tag = match (voice.mode, voice.lang) {
            (Mode::Basic, Lang::Ko) => "basic-ko",
            (Mode::Expert, Lang::En) => "expert-en",
            (Mode::Basic, Lang::En) => "basic-en",
            (Mode::Expert, Lang::Ko) => "expert-ko",
        };
        Self { window, voice, folder, tag, name: name.to_string(), clicks: 0 }
    }

    pub fn say(&self, key: &str) -> &'static str {
        i18n::text(self.voice, key)
    }

    /// What a kind's token is called: "Miner 2".
    pub fn named(&self, kind: Kind, number: u32) -> String {
        format!("{} {number}", self.say(kind_key(kind)))
    }

    pub fn labels(&self) -> Vec<String> {
        self.window.query_all(By::new().include_labels()).filter_map(|node| node.accesskit_node().label()).collect()
    }

    /// The token of this name and its line: "Miner 1, 2 blocks found".
    pub fn token(&self, name: &str) -> Option<String> {
        let line = format!("{name}, ");
        self.labels().into_iter().find(|label| label == name || label.starts_with(&line))
    }

    /// The last number in a token's line: its blocks, shares, payments or what it holds.
    pub fn count(&self, name: &str) -> Option<f64> {
        let label = self.token(name)?;
        let (_, line) = label.split_once(", ")?;
        numbers(line).last().copied()
    }

    /// The numbers in the first line that holds the fixed words of `key`.
    pub fn line_numbers(&self, key: &str) -> Option<Vec<f64>> {
        let template = self.say(key);
        let fixed = template.split("{}").max_by_key(|part| part.len()).unwrap_or(template).trim();
        self.labels().into_iter().find(|label| label.contains(fixed)).map(|label| numbers(&label))
    }

    /// Presses the button with these words, bringing it into view first, as a person scrolls to it.
    pub fn press(&mut self, label: &str) {
        let find = |window: &Window| window.query(By::new().role(Role::Button).label(label)).is_some();
        assert!(find(&self.window), "no button {label:?} on the screen; buttons: {:?}", self.buttons());
        self.window.get_by_role_and_label(Role::Button, label).scroll_to_me();
        self.window.run_steps(2);
        self.window.get_by_role_and_label(Role::Button, label).click();
        self.window.run_steps(2);
        self.clicks += 1;
    }

    pub fn has_button(&self, label: &str) -> bool {
        self.window.query(By::new().role(Role::Button).label(label)).is_some()
    }

    fn buttons(&self) -> Vec<String> {
        self.window.query_all(By::new().role(Role::Button)).filter_map(|node| node.accesskit_node().label()).collect()
    }

    /// One click on "+ kind".
    pub fn add(&mut self, kind: Kind) {
        let label = format!("+ {}", self.say(kind_key(kind)));
        self.press(&label);
    }

    /// Clicks a token on the map: its panel opens, or closes when it was open.
    pub fn select(&mut self, name: &str) {
        let label = self.token(name).unwrap_or_else(|| panic!("no token {name}"));
        self.window.get_by_role_and_label(Role::Button, &label).click();
        self.window.run_steps(2);
        self.clicks += 1;
    }

    /// Drags a token onto another, as a person drags a miner into a pool.
    pub fn drag(&mut self, from: &str, onto: &str) {
        let centre = |run: &Self, name: &str| {
            let label = run.token(name).unwrap_or_else(|| panic!("no token {name}"));
            run.window.get_by_role_and_label(Role::Button, &label).rect().center()
        };
        let (start, end) = (centre(self, from), centre(self, onto));
        self.window.hover_at(start);
        self.window.run_steps(1);
        self.window.drag_at(start);
        self.window.run_steps(1);
        for step in 1..=8 {
            self.window.hover_at(start + (end - start) * (step as f32 / 8.0));
            self.window.run_steps(1);
        }
        self.window.drop_at(end);
        self.window.run_steps(2);
        self.clicks += 1;
    }

    /// Draws frames until `seen`, for at most `limit` seconds of real time.
    pub fn wait(&mut self, limit: u64, what: &str, seen: impl Fn(&Self) -> bool) {
        let start = Instant::now();
        let mut said = Instant::now();
        loop {
            self.window.run_steps(1);
            if seen(self) {
                // What arrived after the frame was drawn is drawn before anyone looks.
                self.window.run_steps(2);
                println!("{what}: after {:.0?}", start.elapsed());
                return;
            }
            if said.elapsed() > Duration::from_secs(10) {
                said = Instant::now();
                let tokens: Vec<String> = self.labels().into_iter().filter(|label| label.contains(", ")).collect();
                println!("  waiting for {what}: {tokens:?}");
            }
            assert!(start.elapsed() < Duration::from_secs(limit), "{what}: not within {limit} s; {:?}", self.labels());
            std::thread::sleep(Duration::from_millis(150));
        }
    }

    /// Keeps a picture with `OLC_SHOTS`: `lab-net-<name>-<voice>-1280`.
    pub fn shoot(&mut self, name: &str) {
        if let Some(folder) = self.folder.clone() {
            shot(&mut self.window, &folder, &format!("lab-net-{name}-{}-1280", self.tag));
        } else {
            self.window.run_steps(SETTLE_STEPS);
        }
    }

    pub fn processes(&self) -> usize {
        processes_of(&self.name)
    }

    /// Waits until no node process of this run is left.
    pub fn none_left(&mut self, what: &str) {
        let start = Instant::now();
        while self.processes() > 0 {
            self.window.run_steps(1);
            assert!(start.elapsed() < Duration::from_secs(15), "{what}: {} node processes left", self.processes());
            std::thread::sleep(Duration::from_millis(200));
        }
    }

    /// Draws frames for `seconds` of real time while the network runs on.
    pub fn pass(&mut self, seconds: u64) {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(seconds) {
            self.window.run_steps(1);
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    /// Closes the app, as a person closes its window, and waits until no node process is left.
    pub fn close(self) {
        let LabRun { window, name, .. } = self;
        drop(window);
        let start = Instant::now();
        while processes_of(&name) > 0 {
            assert!(start.elapsed() < Duration::from_secs(15), "closing the app: {} node processes left", processes_of(&name));
            std::thread::sleep(Duration::from_millis(200));
        }
    }

    /// Elements that reach past the right edge of a window `width` wide.
    pub fn too_wide(&self, width: f32) -> Vec<String> {
        self.window
            .query_all(By::new().include_labels())
            .filter(|node| node.accesskit_node().bounding_box().is_some())
            .filter(|node| node.rect().max.x > width + 1.0 && node.rect().width() > 0.0)
            .map(|node| format!("{:?} {:?}", node.accesskit_node().label(), node.rect()))
            .collect()
    }

    /// Looks at the Lab as it is now at `size` with text at `percent`, and lists what overflows.
    pub fn overflow_at(&mut self, size: (u32, u32), percent: u16) -> Vec<String> {
        self.window.state_mut().settings_mut().text_percent = percent;
        self.window.run_steps(2);
        self.window.set_size(egui::vec2(size.0 as f32, size.1 as f32));
        self.window.run_steps(SETTLE_STEPS);
        let wrong = self.too_wide(size.0 as f32).into_iter().map(|what| format!("{}x{} {percent}%: {what}", size.0, size.1)).collect();
        if let Some(folder) = self.folder.clone() {
            shot(&mut self.window, &folder, &format!("lab-net-fit-{}-{}-{percent}", self.tag, size.0));
        }
        wrong
    }

    pub fn back_to_normal(&mut self) {
        self.window.state_mut().settings_mut().text_percent = 100;
        self.window.run_steps(2);
        self.window.set_size(egui::vec2(SHOT_SIZE.0 as f32, SHOT_SIZE.1 as f32));
        self.window.run_steps(SETTLE_STEPS);
    }
}

/// Node processes of the run named `name` still alive, found by the place in their command line.
/// Linux only: elsewhere none are found.
fn processes_of(name: &str) -> usize {
    let needle = format!("{name}/lab");
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return 0;
    };
    entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().chars().all(|c| c.is_ascii_digit()))
        .filter_map(|entry| std::fs::read(entry.path().join("cmdline")).ok())
        .filter(|line| String::from_utf8_lossy(line).contains(&needle))
        .count()
}

/// The numbers in a line, decimals kept.
pub(super) fn numbers(line: &str) -> Vec<f64> {
    line.split(|c: char| !(c.is_ascii_digit() || c == '.'))
        .map(|part| part.trim_matches('.'))
        .filter(|part| !part.is_empty())
        .filter_map(|part| part.parse().ok())
        .collect()
}
