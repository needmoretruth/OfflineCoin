//! Actions from a terminal. Keys and choices work on a node's folder. The rest need the core, so
//! it starts without a window for as long as the action takes.
use super::commands::Command;
use super::console::answer_to;
use super::folder::Folder;
use super::ops::Book;
use super::service::{Core, CoreLaunch};
use super::snapshot::BoardView;
use super::words::Desk;
use super::{new_coin, write_coin_file};
use crate::fleet::{FleetPort, RealFleet};
use crate::instance::{claim, Instance};
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
    /// The core, with the hold that keeps a second copy off the same wallet and cables.
    core: Option<(Core, Instance)>,
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
            let instance = claim(&root)?.ok_or("OLC Core is open. Use it, or close it first.")?;
            let fleet: Arc<dyn FleetPort> = Arc::new(RealFleet::new(self.ports.clone())?);
            let settings = Settings::load(&root);
            let launch = CoreLaunch {
                paths: Paths { root },
                fleet: Some(fleet),
                usb: true,
                node_program: None,
            };
            self.core = Some((Core::start(launch, settings)?, instance));
        }
        let core = self.core.as_ref().map(|(core, _)| core);
        core.ok_or_else(|| "The core did not start.".to_string())
    }

    /// Hands `command` to the core and waits for what it says.
    fn through_core(&mut self, command: Command) -> Result<String, String> {
        let core = self.core()?;
        answer_to(&|| core.snapshot(), &|one| core.send(one), command)
    }
}

impl Desk for Terminal {
    fn command(&mut self, command: Command) -> Result<String, String> {
        match command {
            Command::Own(order) => self.folder.carry_out(order).map(|(_, detail)| detail),
            Command::NewCoinFile { choices, path } => {
                let coin = new_coin(&choices).map_err(|bad| bad.to_string())?;
                write_coin_file(&coin, &path).map(|()| path)
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
