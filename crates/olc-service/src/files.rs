//! Choosing a file by looking at folders, so nobody has to type a path.
use olc_platform::platform;
pub use olc_platform::{Place, PlaceKind};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

const DAY_MS: u64 = 86_400_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileRow {
    pub name: String,
    pub path: PathBuf,
    pub is_folder: bool,
    pub bytes: u64,
    pub modified_ms: u64,
}

pub fn places() -> Vec<Place> {
    platform().places()
}

fn row_of(path: PathBuf, name: String) -> Option<FileRow> {
    let meta = fs::metadata(&path).ok()?;
    let modified = meta.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
    Some(FileRow {
        name,
        path,
        is_folder: meta.is_dir(),
        bytes: meta.len(),
        modified_ms: u64::try_from(modified.as_millis()).unwrap_or(u64::MAX),
    })
}

/// What is in `folder`: folders first, then files ending in `ending`, newest first. Files that
/// start with a dot stay out, as they do in a file manager.
pub fn rows_in(folder: &Path, ending: &str) -> Result<Vec<FileRow>, String> {
    let entries = fs::read_dir(folder).map_err(|error| error.to_string())?;
    let mut rows: Vec<FileRow> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            (!name.starts_with('.')).then(|| row_of(entry.path(), name)).flatten()
        })
        .filter(|row| row.is_folder || row.name.ends_with(ending))
        .collect();
    rows.sort_by_key(|row| (!row.is_folder, u64::MAX - row.modified_ms));
    Ok(rows)
}

/// Year, month and day of a moment, in UTC. A file name needs no more than that.
fn date_of(ms: u64) -> (u64, u64, u64) {
    let days = ms / DAY_MS + 719_468;
    let era = days / 146_097;
    let day_of_era = days % 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted + 2) / 5 + 1;
    let month = if shifted < 10 { shifted + 3 } else { shifted - 9 };
    let year = year_of_era + era * 400 + u64::from(month <= 2);
    (year, month, day)
}

/// A name no file in `folder` has yet, such as `olc-wallet-2026-11-02.backup`.
pub fn fresh_name(folder: &Path, stem: &str, extension: &str, now_ms: u64) -> String {
    let (year, month, day) = date_of(now_ms);
    let dated = format!("{stem}-{year}-{month:02}-{day:02}");
    let first = format!("{dated}.{extension}");
    if !folder.join(&first).exists() {
        return first;
    }
    (2..)
        .map(|number| format!("{dated}-{number}.{extension}"))
        .find(|name| !folder.join(name).exists())
        .unwrap_or(first)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env::temp_dir;
    use std::process::id;

    #[test]
    fn a_date_reads_as_people_write_it() {
        assert_eq!(date_of(0), (1970, 1, 1));
        assert_eq!(date_of(1_793_577_600_000), (2026, 11, 2));
        assert_eq!(date_of(1_709_164_800_000), (2024, 2, 29));
    }

    #[test]
    fn folders_come_first_and_other_files_stay_out() {
        let folder = temp_dir().join(format!("olc-files-{}", id()));
        fs::create_dir_all(folder.join("inner")).unwrap();
        fs::write(folder.join("wallet.backup"), "x").unwrap();
        fs::write(folder.join("photo.jpg"), "x").unwrap();
        fs::write(folder.join(".hidden.backup"), "x").unwrap();
        let rows = rows_in(&folder, ".backup").unwrap();
        let names: Vec<&str> = rows.iter().map(|row| row.name.as_str()).collect();
        assert_eq!(names, ["inner", "wallet.backup"]);
        fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn a_new_name_never_lands_on_an_old_file() {
        let folder = temp_dir().join(format!("olc-names-{}", id()));
        fs::create_dir_all(&folder).unwrap();
        let first = fresh_name(&folder, "olc-wallet", "backup", 0);
        assert_eq!(first, "olc-wallet-1970-01-01.backup");
        fs::write(folder.join(&first), "x").unwrap();
        assert_eq!(fresh_name(&folder, "olc-wallet", "backup", 0), "olc-wallet-1970-01-01-2.backup");
        fs::remove_dir_all(&folder).unwrap();
    }
}
