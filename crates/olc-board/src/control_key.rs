//! The key that signs orders to boards. It stays on this computer, readable only by its owner.
use olc_crypto::PublicKey;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

const FILE_NAME: &str = "control.key";
const OWNER_ONLY: u32 = 0o600;

pub struct ControlKey {
    secret: [u8; 32],
}

/// `~/.config/olc`, or wherever `OLC_CONFIG` points.
pub fn config_folder() -> Result<PathBuf, String> {
    let folder = match std::env::var_os("OLC_CONFIG") {
        Some(folder) => PathBuf::from(folder),
        None => {
            let home = std::env::var_os("HOME").ok_or("HOME is not set")?;
            PathBuf::from(home).join(".config/olc")
        }
    };
    fs::create_dir_all(&folder).map_err(|error| format!("{}: {error}", folder.display()))?;
    Ok(folder)
}

impl ControlKey {
    pub fn load_or_create(folder: &Path) -> Result<Self, String> {
        let path = folder.join(FILE_NAME);
        let shown = path.display();
        if let Ok(meta) = fs::metadata(&path) {
            if meta.permissions().mode() & 0o777 != OWNER_ONLY {
                return Err(format!("{shown}: only its owner may read this file"));
            }
            let bytes = fs::read(&path).map_err(|error| format!("{shown}: {error}"))?;
            let secret = bytes.try_into().map_err(|_| format!("{shown}: bad key"))?;
            return Ok(Self { secret });
        }
        let mut secret = [0u8; 32];
        getrandom::fill(&mut secret).map_err(|error| error.to_string())?;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(OWNER_ONLY)
            .open(&path)
            .map_err(|error| format!("{shown}: {error}"))?;
        file.write_all(&secret)
            .map_err(|error| format!("{shown}: {error}"))?;
        Ok(Self { secret })
    }

    pub fn secret(&self) -> &[u8; 32] {
        &self.secret
    }

    pub fn public(&self) -> Result<PublicKey, String> {
        olc_crypto::public_key(&self.secret).ok_or_else(|| "The control key is bad".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("olc-key-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn a_new_key_is_private_and_stays_the_same() {
        let path = folder("new");
        let first = ControlKey::load_or_create(&path).unwrap();
        let mode = fs::metadata(path.join(FILE_NAME)).unwrap().permissions();
        assert_eq!(mode.mode() & 0o777, OWNER_ONLY);
        let second = ControlKey::load_or_create(&path).unwrap();
        assert_eq!(first.public(), second.public());
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn a_key_others_can_read_is_refused() {
        let path = folder("open");
        ControlKey::load_or_create(&path).unwrap();
        let file = path.join(FILE_NAME);
        fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(ControlKey::load_or_create(&path).is_err());
        fs::remove_dir_all(path).unwrap();
    }
}
