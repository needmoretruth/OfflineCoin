//! What a person changes on their own node: keys, where mining pays, how much it keeps.
use super::commands::Personal;
use super::explorer::{explore, Explored};
use super::host::Host;
use olc_core::{BlockHeader, Hash};
use olc_node::{Runner, Status};
use olc_store::{
    back_up, format_secret, new_secret, parse_secret, restore, save_wallet, NodeSettings,
};
use olc_wallet::{format_address, parse_address};
use std::fs::OpenOptions;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

const STOPPED: &str = "The node stopped";

impl Host {
    fn on_runner<R>(&self, job: impl FnOnce(&mut Runner) -> R) -> Result<R, String> {
        let mut runner = self.runner.lock().map_err(|_| STOPPED.to_string())?;
        Ok(job(&mut runner))
    }

    fn change_settings(&self, change: impl FnOnce(&mut NodeSettings)) -> Result<(), String> {
        let Some(folder) = self.chain_folder() else {
            return Ok(());
        };
        let mut settings = NodeSettings::load(&folder);
        change(&mut settings);
        settings.save(&folder)
    }

    pub fn new_key(&self) -> Result<String, String> {
        self.add_key(new_secret()?)
    }

    pub fn import_key(&self, text: &str) -> Result<String, String> {
        self.add_key(parse_secret(text).ok_or("That is not a secret key")?)
    }

    fn add_key(&self, secret: [u8; 32]) -> Result<String, String> {
        let folder = self.wallet_folder();
        self.on_runner(|runner| {
            let wallet = runner.engine.wallet_mut();
            let index = wallet.add_secret(secret).map_err(|e| format!("{e:?}"))?;
            save_wallet(&folder, wallet)?;
            let added = wallet.keys().nth(index).map(|(address, _)| address);
            Ok(added.map(|address| format_address(&address)).unwrap_or_default())
        })?
    }

    pub fn use_key(&self, index: usize) -> Result<(), String> {
        let folder = self.wallet_folder();
        self.on_runner(|runner| {
            let wallet = runner.engine.wallet_mut();
            wallet.set_receiving(index).map_err(|e| format!("{e:?}"))?;
            save_wallet(&folder, wallet)
        })?
    }

    /// Writes one key to a file only its owner can read. It never goes to the screen or a log.
    pub fn export_key(&self, index: usize, path: &Path) -> Result<(), String> {
        let secret = self.on_runner(|runner| runner.engine.wallet().export_secret(index))?;
        let secret = secret.ok_or("No such key")?;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
            .map_err(|error| error.to_string())?;
        writeln!(file, "{}", format_secret(&secret)).map_err(|error| error.to_string())
    }

    pub fn back_up(&self, path: &Path, password: Option<&str>) -> Result<(), String> {
        self.on_runner(|runner| back_up(runner.engine.wallet(), path, password))?
    }

    /// Adds the keys in a backup to this wallet and says how many were new.
    pub fn restore(&self, path: &Path, password: Option<&str>) -> Result<usize, String> {
        let backup = restore(path, password)?;
        let folder = self.wallet_folder();
        self.on_runner(|runner| {
            let wallet = runner.engine.wallet_mut();
            let secrets = (0..backup.key_count()).filter_map(|index| backup.export_secret(index));
            let added = secrets.filter(|secret| wallet.add_secret(*secret).is_ok()).count();
            save_wallet(&folder, wallet).map(|()| added)
        })?
    }

    /// Mines to `text`, or to this node's own key when it is empty.
    pub fn set_miner(&self, text: &str) -> Result<String, String> {
        let text = text.trim();
        let miner = match text {
            "" => None,
            text => Some(parse_address(text).ok_or("That is not an address")?),
        };
        let address = self.on_runner(|runner| {
            runner.engine.set_miner(miner);
            format_address(&runner.engine.miner())
        })?;
        self.change_settings(|settings| settings.miner = miner.map(|_| text.to_string()))?;
        Ok(address)
    }

    pub fn set_mine_note(&self, text: &str) -> Result<(), String> {
        self.on_runner(|runner| runner.engine.set_note(text.as_bytes()))?;
        self.change_settings(|settings| settings.note = text.to_string())
    }

    pub fn set_recent(&self, recent: Option<u64>) -> Result<(), String> {
        self.on_runner(|runner| runner.engine.chain_mut().set_recent(recent))?;
        self.change_settings(|settings| settings.recent = recent)
    }

    pub fn set_merging(&self, on: bool) -> Result<(), String> {
        self.on_runner(|runner| runner.engine.set_merging(on))?;
        self.change_settings(|settings| settings.no_merge = !on)
    }

    pub fn write_note(&self, text: &str) -> Result<Hash, String> {
        self.on_runner(|runner| runner.engine.write_note(text.as_bytes()))?
            .map_err(|error| format!("{error:?}"))
    }

    /// Takes the unspent set of the peer that has what this node lacks. Only a person says so.
    pub fn accept_snapshot(&self) -> Result<(), String> {
        self.on_runner(|runner| {
            let status: Status = runner.engine.status();
            let (peer, _) = status.needs_history.ok_or("No peer to take it from")?;
            runner.engine.accept_snapshot(peer);
            Ok(())
        })?
    }

    fn explore(&self, text: &str) -> Explored {
        self.on_runner(|runner| explore(runner.engine.chain(), text))
            .unwrap_or(Explored::Nothing)
    }

    /// Grinds a header from outside in place of mining, or stops with `None`.
    pub fn take_work(&self, work: Option<(BlockHeader, u64)>) {
        self.set_work(work);
    }

    /// Does what a person asked. The answer is a notice's key, its detail, and whether it went well.
    pub fn obey(&self, order: Personal) -> (&'static str, String, bool) {
        let done = match order {
            Personal::NewKey => self.new_key().map(|address| ("notice.key_added", address)),
            Personal::ImportKey(text) => {
                self.import_key(&text).map(|address| ("notice.key_added", address))
            }
            Personal::UseKey(index) => self.use_key(index).map(|()| ("notice.key_chosen", String::new())),
            Personal::ExportKey { index, path } => self
                .export_key(index, Path::new(&path))
                .map(|()| ("notice.key_saved", path)),
            Personal::BackUp { path, password } => self
                .back_up(Path::new(&path), password.as_deref())
                .map(|()| ("notice.backed_up", path)),
            Personal::Restore { path, password } => self
                .restore(Path::new(&path), password.as_deref())
                .map(|added| ("notice.restored", added.to_string())),
            Personal::SetMiner(text) => {
                self.set_miner(&text).map(|address| ("notice.miner_set", address))
            }
            Personal::SetMineNote(text) => {
                self.set_mine_note(&text).map(|()| ("notice.saved", String::new()))
            }
            Personal::SetRecent(recent) => {
                self.set_recent(recent).map(|()| ("notice.saved", String::new()))
            }
            Personal::SetMerging(on) => {
                self.set_merging(on).map(|()| ("notice.saved", String::new()))
            }
            Personal::WriteNote(text) => self.write_note(&text).map(|_| ("notice.noted", text)),
            Personal::AcceptSnapshot => {
                self.accept_snapshot().map(|()| ("notice.snapshot_taking", String::new()))
            }
            Personal::Explore(text) => {
                self.set_explored(self.explore(&text));
                return ("", String::new(), true);
            }
        };
        match done {
            Ok((key, detail)) => (key, detail, true),
            Err(error) => ("notice.not_done", error, false),
        }
    }
}
