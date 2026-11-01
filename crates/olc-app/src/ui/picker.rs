//! Choosing where a file goes or which file to read, by looking. Nobody types a path, and the
//! same sheet works on every system.
use super::floating::{sheet, sheet_head};
use super::tokens::{self, Tokens};
use super::widgets::{button, button_on, cell, field, meta, mono, pick_row, Kind};
use crate::core::unix_ms;
use crate::i18n::{self, Mode, Voice};
use egui::{Label, Layout, Align, RichText, ScrollArea, Ui};
use olc_service::files::{fresh_name, places, rows_in, FileRow, Place, PlaceKind};
use std::path::{Path, PathBuf};

const LIST_HEIGHT: f32 = 240.0;
const PLACES_WIDTH: f32 = 150.0;

enum Job {
    Open { ending: &'static str },
    Save { stem: &'static str, extension: &'static str, name: String },
}

pub struct Picker {
    title: &'static str,
    job: Job,
    folder: PathBuf,
    places: Vec<Place>,
    rows: Result<Vec<FileRow>, String>,
    chosen: Option<PathBuf>,
}

pub enum Picked {
    Waiting,
    Chosen(PathBuf),
    Cancelled,
}

fn first_folder(start: Option<&Path>, places: &[Place]) -> PathBuf {
    let usual = places.iter().find(|place| place.kind == PlaceKind::Documents);
    match (start, usual, places.first()) {
        (Some(start), _, _) => start.to_path_buf(),
        (None, Some(place), _) | (None, None, Some(place)) => place.path.clone(),
        _ => PathBuf::from("."),
    }
}

impl Picker {
    fn new(title: &'static str, job: Job, start: Option<&Path>) -> Self {
        let places = places();
        let folder = first_folder(start, &places);
        let mut picker = Self { title, job, folder, places, rows: Ok(Vec::new()), chosen: None };
        picker.go(picker.folder.clone());
        picker
    }

    /// Picks a file to read, showing only those ending in `ending`.
    pub fn open(title: &'static str, ending: &'static str, start: Option<&Path>) -> Self {
        Self::new(title, Job::Open { ending }, start)
    }

    /// Picks a folder to write in, with a name that is new there.
    pub fn save(title: &'static str, stem: &'static str, extension: &'static str, start: Option<&Path>) -> Self {
        let job = Job::Save { stem, extension, name: String::new() };
        Self::new(title, job, start)
    }

    fn ending(&self) -> &str {
        match &self.job {
            Job::Open { ending } => ending,
            Job::Save { extension, .. } => extension,
        }
    }

    fn go(&mut self, folder: PathBuf) {
        self.rows = rows_in(&folder, self.ending());
        self.chosen = None;
        if let Job::Save { stem, extension, name } = &mut self.job {
            *name = fresh_name(&folder, stem, extension, unix_ms());
        }
        self.folder = folder;
    }

    fn place_rows(&mut self, ui: &mut Ui, t: &Tokens, voice: Voice) {
        let mut go = None;
        for place in &self.places {
            let name = match place.kind {
                PlaceKind::Home => i18n::text(voice, "pick.home").to_string(),
                PlaceKind::Desktop => i18n::text(voice, "pick.desktop").to_string(),
                PlaceKind::Documents => i18n::text(voice, "pick.documents").to_string(),
                PlaceKind::Downloads => i18n::text(voice, "pick.downloads").to_string(),
                PlaceKind::Drive => place.label.clone(),
            };
            let here = place.path == self.folder;
            if pick_row(ui, t, 32.0, here, &name, |ui| cell(ui, name.as_str())).clicked() {
                go = Some(place.path.clone());
            }
        }
        if let Some(folder) = go {
            self.go(folder);
        }
    }

    fn file_rows(&mut self, ui: &mut Ui, t: &Tokens, voice: Voice) {
        let mut go = None;
        let up = self.folder.parent().map(Path::to_path_buf);
        let shown = up.filter(|_| ui.is_enabled());
        ScrollArea::vertical().id_salt("files").max_height(LIST_HEIGHT).auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            if let Some(up) = shown {
                let name = i18n::text(voice, "pick.up");
                if pick_row(ui, t, 32.0, false, name, |ui| cell(ui, RichText::new(name).color(t.soft))).clicked() {
                    go = Some(up);
                }
            }
            match &self.rows {
                Ok(rows) if rows.is_empty() => {
                    ui.add_space(tokens::GAP);
                    meta(ui, i18n::text(voice, "pick.empty"), t);
                }
                Ok(rows) => {
                    for row in rows {
                        let current = self.chosen.as_deref() == Some(row.path.as_path());
                        let clicked = pick_row(ui, t, 32.0, current, &row.name, |ui| {
                            let text = if row.is_folder { RichText::new(&row.name).color(t.soft) } else { RichText::new(&row.name) };
                            cell(ui, text);
                        })
                        .clicked();
                        if clicked && row.is_folder {
                            go = Some(row.path.clone());
                        } else if clicked {
                            self.chosen = Some(row.path.clone());
                        }
                    }
                }
                Err(_) => {
                    meta(ui, i18n::text(voice, "pick.unreadable"), t);
                }
            }
        });
        if let Some(folder) = go {
            self.go(folder);
        }
    }

    /// The answer for this frame.
    pub fn show(&mut self, ui: &Ui, t: &Tokens, voice: Voice) -> Picked {
        let mut picked = Picked::Waiting;
        let title = i18n::text(voice, self.title);
        let (_, closed) = sheet(ui, t, "picker", 620.0, |ui| {
            sheet_head(ui, title);
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(egui::vec2(PLACES_WIDTH, LIST_HEIGHT), Layout::top_down(Align::Min), |ui| {
                    ui.set_width(PLACES_WIDTH);
                    ui.spacing_mut().item_spacing.y = 1.0;
                    self.place_rows(ui, t, voice);
                });
                ui.vertical(|ui| self.file_rows(ui, t, voice));
            });
            ui.add_space(tokens::GAP);
            ui.add(Label::new(mono(self.folder.display().to_string()).color(t.muted)).truncate());
            ui.add_space(tokens::GAP);
            let answer = self.foot(ui, t, voice);
            if !matches!(answer, Picked::Waiting) {
                picked = answer;
            }
        });
        if closed {
            return Picked::Cancelled;
        }
        picked
    }

    fn foot(&mut self, ui: &mut Ui, t: &Tokens, voice: Voice) -> Picked {
        let mut picked = Picked::Waiting;
        let say = |key| i18n::text(voice, key);
        ui.horizontal(|ui| {
            let target = match &mut self.job {
                Job::Save { name, .. } => {
                    if voice.mode == Mode::Expert {
                        field(ui, name, say("pick.name"), 280.0);
                    } else {
                        ui.add(Label::new(name.as_str()).truncate());
                    }
                    (!name.trim().is_empty()).then(|| self.folder.join(name.trim()))
                }
                Job::Open { .. } => self.chosen.clone(),
            };
            let word = if matches!(self.job, Job::Save { .. }) { "pick.save" } else { "pick.open" };
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if button(ui, t, say("common.cancel"), Kind::Plain).clicked() {
                    picked = Picked::Cancelled;
                }
                if button_on(ui, t, say(word), Kind::Main, target.is_some()).clicked() {
                    if let Some(target) = target {
                        picked = Picked::Chosen(target);
                    }
                }
            });
        });
        picked
    }
}
