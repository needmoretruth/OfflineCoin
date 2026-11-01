//! A node's folder when no node runs on it. Keys and choices wait there for the next start.
use super::ops::Book;
use olc_node::Runner;
use olc_store::{load_wallet, save_wallet, NodeSettings};
use olc_wallet::Wallet;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;

pub struct Folder {
    pub directory: PathBuf,
    /// Where a node on this computer takes notes.
    pub web_port: u16,
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

    fn write_note(&self, text: &str) -> Result<String, String> {
        let body = serde_json::json!({ "text": text }).to_string();
        let mut stream = TcpStream::connect(("127.0.0.1", self.web_port))
            .map_err(|_| "No node runs on this computer.")?;
        let request = format!(
            "POST /api/note HTTP/1.0\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(request.as_bytes()).map_err(|error| error.to_string())?;
        let mut reply = String::new();
        stream.read_to_string(&mut reply).map_err(|error| error.to_string())?;
        Ok(reply.split("\r\n\r\n").nth(1).unwrap_or(&reply).to_string())
    }

    fn show_explored(&self, _text: &str) -> Result<(), String> {
        Err("That needs a running node.".into())
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
        Folder { directory, web_port: 1 }
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
