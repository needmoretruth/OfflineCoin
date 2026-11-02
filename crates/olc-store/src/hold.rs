//! One node on a folder. Two writing one chain would spoil it, and a key made beside a running node
//! would be written over, because the node keeps its wallet in memory. The system lets go of a
//! hold when its program ends, however it ends.
use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};

const HOLD_FILE: &str = "node.hold";
const WALLET_HOLD: &str = "wallet.hold";
/// What a second node, or a change to a wallet in use, is told.
pub const NODE_ON_FOLDER: &str = "A node is running on this folder.";

pub struct FolderHold {
    folder: PathBuf,
    _file: File,
}

/// Moves a node's hold to `directory`. A node changing kind in its own folder keeps what it has.
pub fn hold_folder(held: &mut Option<FolderHold>, directory: &Path) -> Result<(), String> {
    if held.as_ref().is_some_and(|hold| hold.folder == directory) {
        return Ok(());
    }
    fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    let mut options = OpenOptions::new();
    options.create(true).truncate(false).write(true);
    let file = options.open(directory.join(HOLD_FILE)).map_err(|error| error.to_string())?;
    file.try_lock().map_err(|_| NODE_ON_FOLDER)?;
    *held = Some(FolderHold { folder: directory.to_path_buf(), _file: file });
    Ok(())
}

/// Says a node keeps the wallet in a folder in memory, for as long as this lives.
pub struct WalletHold {
    _file: File,
}

/// Several nodes may share one wallet, so this never turns a node away.
pub fn hold_wallet(directory: &Path) -> Result<WalletHold, String> {
    fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    let mut options = OpenOptions::new();
    options.create(true).truncate(false).write(true);
    let file = options.open(directory.join(WALLET_HOLD)).map_err(|error| error.to_string())?;
    // This waits only for the short look `node_runs_on` takes.
    file.lock_shared().map_err(|error| error.to_string())?;
    Ok(WalletHold { _file: file })
}

/// True while a running node holds `directory`, for its chain or for its wallet.
pub fn node_runs_on(directory: &Path) -> bool {
    [HOLD_FILE, WALLET_HOLD].iter().any(|name| {
        File::open(directory.join(name)).is_ok_and(|file| file.try_lock().is_err())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_serves_one_node_at_a_time() {
        let folder = std::env::temp_dir().join(format!("olc-held-{}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        assert!(!node_runs_on(&folder));

        let mut first = None;
        hold_folder(&mut first, &folder).unwrap();
        assert!(node_runs_on(&folder));
        hold_folder(&mut first, &folder).unwrap();

        let mut second = None;
        assert_eq!(hold_folder(&mut second, &folder).err().as_deref(), Some(NODE_ON_FOLDER));
        drop(first);
        assert!(!node_runs_on(&folder));
        assert!(hold_folder(&mut second, &folder).is_ok());
        let _ = fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_wallet_in_use_shows_while_any_node_shares_it() {
        let folder = std::env::temp_dir().join(format!("olc-shared-{}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        assert!(!node_runs_on(&folder));

        let first = hold_wallet(&folder).unwrap();
        let second = hold_wallet(&folder).unwrap();
        assert!(node_runs_on(&folder));
        drop(first);
        assert!(node_runs_on(&folder));
        drop(second);
        assert!(!node_runs_on(&folder));
        let _ = fs::remove_dir_all(&folder);
    }
}
