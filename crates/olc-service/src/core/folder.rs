//! A node's folder when no node runs on it. Keys and choices wait there for the next start.
use super::ops::{Book, NO_NODE};
use olc_node::Runner;
use olc_store::{load_wallet, save_wallet, NodeSettings};
use olc_wallet::Wallet;
use std::path::PathBuf;

pub struct Folder {
    pub directory: PathBuf,
}

impl Book for Folder {
    fn wallet_view<R>(&self, job: impl FnOnce(&Wallet) -> R) -> Result<R, String> {
        Ok(job(&load_wallet(&self.directory)?))
    }

    fn wallet_change<R>(
        &self,
        job: impl FnOnce(&mut Wallet) -> Result<R, String>,
    ) -> Result<R, String> {
        let mut wallet = load_wallet(&self.directory)?;
        let done = job(&mut wallet)?;
        save_wallet(&self.directory, &wallet).map(|()| done)
    }

    fn live<R>(&self, _job: impl FnOnce(&mut Runner) -> R) -> Result<Option<R>, String> {
        Ok(None)
    }

    fn change_settings(&self, change: impl FnOnce(&mut NodeSettings)) -> Result<(), String> {
        let mut settings = NodeSettings::load(&self.directory);
        change(&mut settings);
        settings.save(&self.directory)
    }

    fn write_note(&self, _text: &str) -> Result<String, String> {
        Err(NO_NODE.into())
    }

    fn prepared(&self, _to: &str, _units: u64, _fee: u64) -> Result<String, String> {
        Err(NO_NODE.into())
    }

    fn send_signed(&self, _signed: &str) -> Result<String, String> {
        Err(NO_NODE.into())
    }

    fn show_explored(&self, _text: &str) -> Result<(), String> {
        Err(NO_NODE.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::Personal;
    use std::env::temp_dir;
    use std::fs;
    use std::process::id;

    fn folder(name: &str) -> Folder {
        let directory = temp_dir().join(format!("olc-folder-{}-{name}", id()));
        fs::create_dir_all(&directory).unwrap();
        Folder { directory }
    }

    #[test]
    fn a_folder_takes_the_same_orders_as_a_running_node() {
        let book = folder("orders");
        let (_, address) = book.carry_out(Personal::NewKey).unwrap();
        let (_, keys) = book.carry_out(Personal::ListKeys).unwrap();
        assert!(keys.contains(&address), "{keys}");
        book.carry_out(Personal::SetRecent(Some(100))).unwrap();
        book.carry_out(Personal::SetMerging(false)).unwrap();
        let settings = NodeSettings::load(&book.directory);
        assert_eq!(settings.recent, Some(100));
        assert!(settings.no_merge);
        assert!(book.carry_out(Personal::AcceptSnapshot).is_err());
        fs::remove_dir_all(&book.directory).unwrap();
    }
}
