//! What a person changes on their own node: keys, where mining pays, how much it keeps.
use super::commands::Personal;
use super::radios;
use super::explorer::{explore, Explored};
use super::host::Host;
use olc_core::{BlockHeader, Tx};
use olc_node::{Rank, Runner, Status};
use olc_store::{
    back_up, format_secret, new_secret, parse_secret, restore, save_wallet, NodeSettings,
    NEEDS_PASSWORD, NOT_A_BACKUP, WRONG_PASSWORD,
};
use olc_platform::platform;
use olc_wallet::{format_address, parse_address, Unsigned, Wallet};
use serde_json::{json, Value};
use std::fs;
use std::path::Path;

const STOPPED: &str = "The node stopped";
const NO_NODE: &str = "No node runs here";

/// Writes `text` for the next computer, or gives it back when no file was asked for.
fn deliver(text: String, save: Option<&str>) -> Result<String, String> {
    let Some(path) = save else {
        return Ok(text);
    };
    fs::write(path, text).map_err(|error| format!("{path}: {error}"))?;
    Ok(path.to_string())
}

/// A node answers in JSON, and says so when it refused.
fn checked(answer: String) -> Result<String, String> {
    let error = serde_json::from_str::<Value>(&answer).ok().map(|value| value["error"].clone());
    match error.as_ref().and_then(Value::as_str) {
        Some(error) => Err(error.to_string()),
        None => Ok(answer),
    }
}

/// The notice that says in plain words that `order` failed. The raw reason stays in its detail.
fn failure_of(order: &Personal) -> &'static str {
    match order {
        Personal::NewKey | Personal::ListKeys | Personal::UseKey(_) => "notice.key_failed",
        Personal::ImportKey(_) => "notice.import_failed",
        Personal::ExportKey { .. } => "notice.export_failed",
        Personal::BackUp { .. } => "notice.backup_failed",
        Personal::Restore { .. } => "notice.restore_failed",
        Personal::Prepare { .. } | Personal::Sign { .. } | Personal::Broadcast(_) => {
            "notice.offline_failed"
        }
        Personal::WriteNote(_) => "notice.note_failed",
        Personal::AcceptSnapshot => "notice.snapshot_failed",
        Personal::Watch(_) | Personal::SetMiner(_) => "notice.bad_address",
        _ => "notice.not_changed",
    }
}

/// What an order needs from the place it lands: a running node, or a node's folder.
pub trait Book {
    fn wallet_view<R>(&self, job: impl FnOnce(&Wallet) -> R) -> Result<R, String>;
    /// Changes the wallet and saves it.
    fn wallet_change<R>(
        &self,
        job: impl FnOnce(&mut Wallet) -> Result<R, String>,
    ) -> Result<R, String>;
    /// Runs `job` on the running node, or gives `None` where none runs.
    fn live<R>(&self, job: impl FnOnce(&mut Runner) -> R) -> Result<Option<R>, String>;
    fn change_settings(&self, change: impl FnOnce(&mut NodeSettings)) -> Result<(), String>;
    /// Writes a note into a block. The answer is what to show for it.
    fn write_note(&self, text: &str) -> Result<String, String>;
    fn show_explored(&self, text: &str) -> Result<(), String>;
    /// Asks the node `olc node` runs on this computer.
    fn ask_node(&self, path: &str, body: &str) -> Result<String, String>;

    /// Follows an address without its key, now and after a restart.
    fn watch(&self, text: &str) -> Result<(), String> {
        let text = text.trim();
        let address = parse_address(text).ok_or("That is not an address.")?;
        self.live(|runner| runner.engine.watch(address))?;
        self.change_settings(|settings| {
            if !settings.watched.iter().any(|known| known == text) {
                settings.watched.push(text.to_string());
            }
        })
    }

    /// Signs a payment read from a file. Touches no network.
    fn sign_file(&self, path: &str, save: Option<&str>) -> Result<String, String> {
        let text = fs::read_to_string(path).map_err(|error| format!("{path}: {error}"))?;
        let answer: serde_json::Value =
            serde_json::from_str(&text).map_err(|error| format!("{path}: {error}"))?;
        let unsigned: Unsigned = serde_json::from_value(answer["unsigned"].clone())
            .map_err(|error| format!("{path}: {error}"))?;
        let signed = self
            .wallet_view(|wallet| wallet.sign(&unsigned))?
            .map_err(|error| format!("{error:?}"))?;
        deliver(serde_json::to_string(&signed).map_err(|error| error.to_string())?, save)
    }

    /// A payment from the watched addresses, ready for a wallet elsewhere to sign.
    fn prepare(&self, to: &str, units: u64, fee: u64, save: Option<&str>) -> Result<String, String> {
        let body = json!({ "to": to, "amount": units, "fee": fee }).to_string();
        deliver(checked(self.ask_node("prepare", &body)?)?, save)
    }

    fn broadcast(&self, path: &str) -> Result<String, String> {
        let body = fs::read_to_string(path).map_err(|error| format!("{path}: {error}"))?;
        checked(self.ask_node("broadcast", body.trim())?)
    }

    fn new_key(&self) -> Result<String, String> {
        self.add_key(new_secret()?)
    }

    fn import_key(&self, text: &str) -> Result<String, String> {
        self.add_key(parse_secret(text.trim()).ok_or("That is not a secret key")?)
    }

    fn add_key(&self, secret: [u8; 32]) -> Result<String, String> {
        self.wallet_change(|wallet| {
            let index = wallet.add_secret(secret).map_err(|e| format!("{e:?}"))?;
            let added = wallet.keys().nth(index).map(|(address, _)| address);
            Ok(added.map(|address| format_address(&address)).unwrap_or_default())
        })
    }

    fn use_key(&self, index: usize) -> Result<(), String> {
        self.wallet_change(|wallet| wallet.set_receiving(index).map_err(|e| format!("{e:?}")))
    }

    /// One line for each key, with a star on the one that receives.
    fn list_keys(&self) -> Result<String, String> {
        self.wallet_view(|wallet| {
            let rows = wallet.keys().enumerate().map(|(index, (address, public))| {
                let mark = if index == wallet.receiving() { "*" } else { " " };
                let public: String = public.iter().map(|byte| format!("{byte:02x}")).collect();
                format!("{mark} {index}  {}  {public}", format_address(&address))
            });
            rows.collect::<Vec<_>>().join("\n")
        })
    }

    /// Writes one key to a file only its owner can read. It never goes to the screen or a log.
    fn export_key(&self, index: usize, path: &Path) -> Result<(), String> {
        let secret = self.wallet_view(|wallet| wallet.export_secret(index))?;
        let secret = secret.ok_or("No such key")?;
        let text = format!("{}\n", format_secret(&secret));
        platform().write_private(path, text.as_bytes()).map_err(|error| error.to_string())
    }

    fn back_up(&self, path: &Path, password: Option<&str>) -> Result<(), String> {
        // A password's hash takes a while, and mining should not wait for it.
        let wallet = self.wallet_view(Wallet::clone)?;
        back_up(&wallet, path, password)
    }

    /// Adds the keys in a backup to this wallet and says how many were new.
    fn restore(&self, path: &Path, password: Option<&str>) -> Result<usize, String> {
        let backup = restore(path, password)?;
        self.wallet_change(|wallet| {
            let secrets = (0..backup.key_count()).filter_map(|index| backup.export_secret(index));
            Ok(secrets.filter(|secret| wallet.add_secret(*secret).is_ok()).count())
        })
    }

    /// Mines to `text`, or to this node's own key when it is empty.
    fn set_miner(&self, text: &str) -> Result<String, String> {
        let text = text.trim();
        let miner = match text {
            "" => None,
            text => Some(parse_address(text).ok_or("That is not an address")?),
        };
        let address = self.live(|runner| {
            runner.engine.set_miner(miner);
            format_address(&runner.engine.miner())
        })?;
        self.change_settings(|settings| settings.miner = miner.map(|_| text.to_string()))?;
        Ok(address.unwrap_or_else(|| text.to_string()))
    }

    fn set_mine_note(&self, text: &str) -> Result<(), String> {
        self.live(|runner| runner.engine.set_note(text.as_bytes()))?;
        self.change_settings(|settings| settings.note = text.to_string())
    }

    fn set_recent(&self, recent: Option<u64>) -> Result<(), String> {
        self.live(|runner| runner.engine.chain_mut().set_recent(recent))?;
        self.change_settings(|settings| settings.recent = recent)
    }

    fn set_links(&self, rank: Rank) -> Result<(), String> {
        self.live(|runner| runner.set_rank(rank))?;
        self.change_settings(|settings| settings.links = rank.text())
    }

    fn set_merging(&self, on: bool) -> Result<(), String> {
        self.live(|runner| runner.engine.set_merging(on))?;
        self.change_settings(|settings| settings.no_merge = !on)
    }

    /// Takes the unspent set of the peer that has what this node lacks. Only a person says so.
    fn accept_snapshot(&self) -> Result<(), String> {
        self.live(|runner| {
            let status: Status = runner.engine.status();
            let (peer, _) = status.needs_history.ok_or("No peer to take it from")?;
            runner.engine.accept_snapshot(peer);
            Ok(())
        })?
        .unwrap_or_else(|| Err(NO_NODE.to_string()))
    }

    /// Does what a person asked. The answer is a notice's key and its detail.
    fn carry_out(&self, order: Personal) -> Result<(&'static str, String), String> {
        match order {
            Personal::NewKey => self.new_key().map(|address| ("notice.key_added", address)),
            Personal::ListKeys => self.list_keys().map(|keys| ("notice.keys", keys)),
            Personal::ImportKey(text) => {
                self.import_key(&text).map(|address| ("notice.key_added", address))
            }
            Personal::UseKey(index) => {
                self.use_key(index).map(|()| ("notice.key_chosen", String::new()))
            }
            Personal::ExportKey { index, path } => {
                self.export_key(index, Path::new(&path)).map(|()| ("notice.key_saved", path))
            }
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
            Personal::SetBluetooth(on) if on && !radios::bluetooth_here() => {
                Err(radios::NO_BLUETOOTH.to_string())
            }
            Personal::SetBluetooth(on) => self
                .change_settings(|settings| settings.bluetooth = on)
                .map(|()| ("notice.saved", String::new())),
            Personal::SetEspNowCard(card) => self
                .change_settings(|settings| settings.esp_now_card = card)
                .map(|()| ("notice.saved", String::new())),
            Personal::SetLinks(rank) => {
                self.set_links(rank).map(|()| ("notice.saved", String::new()))
            }
            Personal::SetMerging(on) => {
                self.set_merging(on).map(|()| ("notice.saved", String::new()))
            }
            Personal::Watch(text) => self.watch(&text).map(|()| ("notice.saved", String::new())),
            Personal::SetLight(on) => self
                .change_settings(|settings| settings.light = on)
                .map(|()| ("notice.saved", String::new())),
            Personal::SetMining(on) => self
                .change_settings(|settings| settings.mining = on)
                .map(|()| ("notice.saved", String::new())),
            Personal::Prepare { to, units, fee, save } => self
                .prepare(&to, units, fee, save.as_deref())
                .map(|answer| ("notice.prepared", answer)),
            Personal::Sign { path, save } => self
                .sign_file(&path, save.as_deref())
                .map(|signed| ("notice.signed", signed)),
            Personal::Broadcast(path) => {
                self.broadcast(&path).map(|answer| ("notice.broadcast", answer))
            }
            Personal::WriteNote(text) => self.write_note(&text).map(|_| ("notice.noted", text)),
            Personal::AcceptSnapshot => {
                self.accept_snapshot().map(|()| ("notice.snapshot_taking", String::new()))
            }
            Personal::Explore(text) => self.show_explored(&text).map(|()| ("", String::new())),
        }
    }
}

impl Host {
    fn on_runner<R>(&self, job: impl FnOnce(&mut Runner) -> R) -> Result<R, String> {
        let mut runner = self.runner.lock().map_err(|_| STOPPED.to_string())?;
        Ok(job(&mut runner))
    }

    /// Grinds a header from outside in place of mining, or stops with `None`.
    pub fn take_work(&self, work: Option<(BlockHeader, u64)>) {
        self.set_work(work);
    }

    /// Does what a person asked. The answer is a notice's key, its detail, and whether it went well.
    pub fn obey(&self, order: Personal) -> (&'static str, String, bool) {
        let failed = failure_of(&order);
        match self.carry_out(order) {
            Ok((key, detail)) => (key, detail, true),
            Err(error) => {
                let key = match error.as_str() {
                    WRONG_PASSWORD => "notice.wrong_password",
                    NEEDS_PASSWORD => "notice.needs_password",
                    NOT_A_BACKUP => "notice.not_a_backup",
                    _ => failed,
                };
                (key, error, false)
            }
        }
    }

    /// A payment built on this node's chain from what it watches.
    fn prepare_here(&self, body: &str) -> Result<String, String> {
        let request: Value = serde_json::from_str(body).map_err(|error| error.to_string())?;
        let to = request["to"].as_str().and_then(parse_address).ok_or("That is not an address.")?;
        let amount = request["amount"].as_u64().filter(|amount| *amount > 0).ok_or("No amount")?;
        let fee = request["fee"].as_u64().unwrap_or(0);
        let unsigned = self
            .on_runner(|runner| runner.engine.prepare(to, amount, fee))?
            .map_err(|error| format!("{error:?}"))?;
        Ok(json!({ "unsigned": unsigned }).to_string())
    }

    fn broadcast_here(&self, body: &str) -> Result<String, String> {
        let tx: Tx = serde_json::from_str(body).map_err(|error| error.to_string())?;
        let id = self.on_runner(|runner| runner.engine.send_signed(tx))?;
        Ok(json!({ "id": id.iter().map(|byte| format!("{byte:02x}")).collect::<String>() }).to_string())
    }
}

impl Book for Host {
    fn wallet_view<R>(&self, job: impl FnOnce(&Wallet) -> R) -> Result<R, String> {
        self.on_runner(|runner| job(runner.engine.wallet()))
    }

    fn wallet_change<R>(
        &self,
        job: impl FnOnce(&mut Wallet) -> Result<R, String>,
    ) -> Result<R, String> {
        let folder = self.wallet_folder();
        self.on_runner(|runner| {
            let wallet = runner.engine.wallet_mut();
            let done = job(wallet)?;
            save_wallet(&folder, wallet).map(|()| done)
        })?
    }

    fn live<R>(&self, job: impl FnOnce(&mut Runner) -> R) -> Result<Option<R>, String> {
        self.on_runner(job).map(Some)
    }

    fn change_settings(&self, change: impl FnOnce(&mut NodeSettings)) -> Result<(), String> {
        let Some(folder) = self.chain_folder() else {
            return Ok(());
        };
        let mut settings = NodeSettings::load(&folder);
        change(&mut settings);
        settings.save(&folder)
    }

    fn write_note(&self, text: &str) -> Result<String, String> {
        self.on_runner(|runner| runner.engine.write_note(text.as_bytes()))?
            .map(|_| text.to_string())
            .map_err(|error| format!("{error:?}"))
    }

    fn ask_node(&self, path: &str, body: &str) -> Result<String, String> {
        match path {
            "prepare" => self.prepare_here(body),
            "broadcast" => self.broadcast_here(body),
            _ => Err(NO_NODE.to_string()),
        }
    }

    fn show_explored(&self, text: &str) -> Result<(), String> {
        let explored = self
            .on_runner(|runner| explore(runner.engine.chain(), text))
            .unwrap_or(Explored::Nothing);
        self.set_explored(explored);
        Ok(())
    }
}
