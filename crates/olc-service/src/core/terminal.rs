//! Actions from a terminal. Keys and choices work on a node's folder. The rest need the core, so
//! it starts without a window for as long as the action takes.
use super::commands::Command;
use super::folder::Folder;
use super::ops::Book;
use super::service::{Core, CoreLaunch};
use super::snapshot::BoardView;
use super::words::Desk;
use super::{new_coin, write_coin_file};
use crate::fleet::{FleetPort, RealFleet};
use crate::settings::{standard_folder, Paths, Settings};
use olc_board::carried::firmware_path;
use olc_board::control_key::{config_folder, ControlKey};
use olc_board::install::install;
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

/// Time for boards to show up after the core starts.
const BOARDS_APPEAR: Duration = Duration::from_secs(8);
/// A board answers within this, or never.
const ANSWER_WAIT: Duration = Duration::from_secs(12);
const LOOK_EVERY: Duration = Duration::from_millis(100);

pub struct Terminal {
    pub folder: Folder,
    /// The app's folder, for the actions that need the core.
    pub root: Option<PathBuf>,
    /// Ports a person named. Empty finds every board by its USB id.
    pub ports: Vec<String>,
    pub firmware: Option<String>,
    /// Asked for before anything runs, so this stays free of prompts.
    pub given_password: Option<String>,
    core: Option<Core>,
}

impl Terminal {
    pub fn new(folder: Folder) -> Self {
        Self {
            folder,
            root: None,
            ports: Vec::new(),
            firmware: None,
            given_password: None,
            core: None,
        }
    }

    fn core(&mut self) -> Result<&Core, String> {
        if self.core.is_none() {
            let root = match &self.root {
                Some(root) => root.clone(),
                None => standard_folder()?,
            };
            std::fs::create_dir_all(&root).map_err(|error| error.to_string())?;
            let fleet: Arc<dyn FleetPort> = Arc::new(RealFleet::new(self.ports.clone())?);
            let settings = Settings::load(&root);
            let launch = CoreLaunch {
                paths: Paths { root },
                fleet: Some(fleet),
                usb: true,
                node_program: None,
            };
            self.core = Some(Core::start(launch, settings)?);
        }
        self.core.as_ref().ok_or_else(|| "The core did not start.".to_string())
    }

    /// Hands `command` to the core and waits for what it says.
    fn through_core(&mut self, command: Command) -> Result<String, String> {
        let asks_board = matches!(command, Command::Board { .. });
        let core = self.core()?;
        let number = core.snapshot().notice.as_ref().map_or(0, |notice| notice.number);
        core.send(command);
        let started = Instant::now();
        let wait = if asks_board { ANSWER_WAIT } else { LOOK_EVERY * 20 };
        while started.elapsed() < wait {
            let snapshot = core.snapshot();
            if let Some(notice) = snapshot.notice.as_ref().filter(|notice| notice.number > number) {
                let said = notice.key.trim_start_matches("notice.").replace('_', " ");
                if !notice.good {
                    return Err(format!("{said}: {}", notice.detail));
                }
                let mut answers = snapshot.boards.iter().rev();
                let answer = answers.find_map(|view| view.answer.as_deref());
                return Ok(answer.unwrap_or(&notice.detail).to_string());
            }
            thread::sleep(LOOK_EVERY);
        }
        Ok(String::new())
    }
}

impl Desk for Terminal {
    fn command(&mut self, command: Command) -> Result<String, String> {
        match command {
            Command::Own(order) => self.folder.carry_out(order).map(|(_, detail)| detail),
            Command::NewCoinFile { choices, path } => {
                write_coin_file(&new_coin(choices), &path).map(|()| path)
            }
            Command::InstallBoard { path, name } => {
                let key = ControlKey::load_or_create(&config_folder()?)?.public()?;
                install(&path, &name, &firmware_path(self.firmware.as_deref()), key)?;
                Ok(name)
            }
            command => self.through_core(command),
        }
    }

    fn boards(&mut self) -> Result<Vec<BoardView>, String> {
        let core = self.core()?;
        let started = Instant::now();
        loop {
            let boards = core.snapshot().boards.clone();
            if !boards.is_empty() || started.elapsed() >= BOARDS_APPEAR {
                return Ok(boards);
            }
            thread::sleep(LOOK_EVERY);
        }
    }

    fn password(&mut self) -> Result<String, String> {
        self.given_password.take().ok_or_else(|| "No password was given.".to_string())
    }
}

/// The running app is a desk too, for a console inside it. A notice carries the outcome.
impl Desk for Core {
    fn command(&mut self, command: Command) -> Result<String, String> {
        self.send(command);
        Ok(String::new())
    }

    fn boards(&mut self) -> Result<Vec<BoardView>, String> {
        Ok(self.snapshot().boards.clone())
    }

    fn password(&mut self) -> Result<String, String> {
        Err("Use the backup page for a password.".into())
    }
}
