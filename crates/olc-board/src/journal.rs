//! What an experiment changed on a board, written down before it changes anything. A run can die
//! by any path, so the way back cannot live in the run.
use crate::control_key::config_folder;
use olc_platform::platform;
use std::fs;
use std::path::PathBuf;

const FILE: &str = "changed-boards";

/// The lines that put one board back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub address: String,
    /// The program that made the change, which may still be busy with it.
    pub owner: u32,
    pub lines: Vec<String>,
}

impl Entry {
    fn to_line(&self) -> String {
        let mut words = vec![self.address.clone(), self.owner.to_string()];
        words.extend(self.lines.iter().cloned());
        words.join("\t")
    }

    fn from_line(line: &str) -> Option<Self> {
        let mut words = line.split('\t');
        Some(Self {
            address: words.next()?.to_string(),
            owner: words.next()?.parse().ok()?,
            lines: words.map(String::from).collect(),
        })
    }

    /// Another program that is still running owns this change.
    pub fn is_held_elsewhere(&self) -> bool {
        self.owner != std::process::id() && platform().process_exists(self.owner)
    }
}

pub struct Journal {
    path: PathBuf,
}

impl Journal {
    pub fn standard() -> Result<Self, String> {
        Ok(Self::at(config_folder()?.join(FILE)))
    }

    pub fn at(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn pending(&self) -> Vec<Entry> {
        let text = fs::read_to_string(&self.path).unwrap_or_default();
        text.lines().filter_map(Entry::from_line).collect()
    }

    fn save(&self, entries: &[Entry]) -> Result<(), String> {
        if entries.is_empty() {
            let _ = fs::remove_file(&self.path);
            return Ok(());
        }
        let lines: Vec<String> = entries.iter().map(Entry::to_line).collect();
        let fresh = self.path.with_extension("new");
        fs::write(&fresh, lines.join("\n") + "\n")
            .and_then(|()| fs::rename(&fresh, &self.path))
            .map_err(|error| error.to_string())
    }

    /// Notes the way back for a board. An older note stands: it holds the true first state.
    pub fn remember(&self, address: &str, lines: Vec<String>) -> Result<(), String> {
        let mut entries = self.pending();
        if entries.iter().any(|entry| entry.address == address) {
            return Ok(());
        }
        entries.push(Entry {
            address: address.to_string(),
            owner: std::process::id(),
            lines,
        });
        self.save(&entries)
    }

    pub fn forget(&self, address: &str) {
        let mut entries = self.pending();
        entries.retain(|entry| entry.address != address);
        let _ = self.save(&entries);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn journal(name: &str) -> Journal {
        let path = std::env::temp_dir().join(format!("olc-journal-{}-{name}", std::process::id()));
        let _ = fs::remove_file(&path);
        Journal::at(path)
    }

    #[test]
    fn a_note_outlives_the_program_and_can_be_forgotten() {
        let notes = journal("kept");
        notes
            .remember("olc1", vec!["limit default".into()])
            .unwrap();
        notes
            .remember("olc2", vec!["limit 5".into(), "offline off".into()])
            .unwrap();
        let again = Journal::at(notes.path.clone());
        assert_eq!(again.pending().len(), 2);
        assert_eq!(again.pending()[1].lines, ["limit 5", "offline off"]);
        again.forget("olc1");
        again.forget("olc2");
        assert!(notes.pending().is_empty());
        assert!(!notes.path.exists());
    }

    #[test]
    fn the_first_state_stands() {
        let notes = journal("first");
        notes.remember("olc1", vec!["limit 1".into()]).unwrap();
        notes.remember("olc1", vec!["limit 2".into()]).unwrap();
        assert_eq!(notes.pending()[0].lines, ["limit 1"]);
        notes.forget("olc1");
    }

    #[test]
    fn this_program_does_not_hold_its_own_notes_away() {
        let notes = journal("own");
        notes.remember("olc1", Vec::new()).unwrap();
        assert!(!notes.pending()[0].is_held_elsewhere());
        notes.forget("olc1");
    }
}
