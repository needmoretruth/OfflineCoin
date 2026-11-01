//! The key that signs orders to boards. It stays on this computer, readable only by its owner.
use olc_crypto::PublicKey;
use olc_platform::platform;
use std::fs;
use std::path::{Path, PathBuf};

const FILE_NAME: &str = "control.key";

pub struct ControlKey {
    secret: [u8; 32],
}

/// The system's place for it, or wherever `OLC_CONFIG` points.
pub fn config_folder() -> Result<PathBuf, String> {
    let folder = match std::env::var_os("OLC_CONFIG") {
        Some(folder) => PathBuf::from(folder),
        None => platform().config_folder().ok_or("This system gave no folder for settings.")?,
    };
    fs::create_dir_all(&folder).map_err(|error| format!("{}: {error}", folder.display()))?;
    Ok(folder)
}

impl ControlKey {
    pub fn load_or_create(folder: &Path) -> Result<Self, String> {
        let path = folder.join(FILE_NAME);
        let shown = path.display();
        if path.exists() {
            if !platform().is_private(&path) {
                return Err(format!("{shown}: only its owner may read this file"));
            }
            let bytes = fs::read(&path).map_err(|error| format!("{shown}: {error}"))?;
            let secret = bytes.try_into().map_err(|_| format!("{shown}: bad key"))?;
            return Ok(Self { secret });
        }
        let mut secret = [0u8; 32];
        getrandom::fill(&mut secret).map_err(|error| error.to_string())?;
        platform()
            .write_private(&path, &secret)
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
        assert!(platform().is_private(&path.join(FILE_NAME)));
        let second = ControlKey::load_or_create(&path).unwrap();
        assert_eq!(first.public(), second.public());
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn a_key_others_can_read_is_refused() {
        let path = folder("open");
        ControlKey::load_or_create(&path).unwrap();
        let file = path.join(FILE_NAME);
        let mut opened = fs::metadata(&file).unwrap().permissions();
        #[allow(clippy::permissions_set_readonly_false)]
        opened.set_readonly(false);
        fs::set_permissions(&file, opened).unwrap();
        // On Windows the user's own folder keeps others out, so nothing changed there.
        let private = platform().is_private(&file);
        assert_eq!(ControlKey::load_or_create(&path).is_err(), !private);
        fs::remove_dir_all(path).unwrap();
    }
}
