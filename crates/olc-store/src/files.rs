//! A computer's [`Medium`]: files of a fixed size, one after another. Dropping old blocks is
//! deleting the oldest file.
use olc_core::medium::{Keep, Medium, SECTOR};
use olc_core::{BlockHeader, Shelf};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

const SEGMENT: u64 = 64 * 1024 * 1024;
const OPEN_AT_ONCE: usize = 4;
const EVERY_HEADER: &str = "every-header.bin";

pub struct Segments {
    directory: PathBuf,
    prefix: &'static str,
    limit: Option<u64>,
    /// Open files by segment number. Writing keeps its file here too.
    open: RefCell<BTreeMap<u64, File>>,
    written: Option<u64>,
}

impl Segments {
    pub fn new(directory: &Path, prefix: &'static str, limit: Option<u64>) -> Self {
        Self {
            directory: directory.to_path_buf(),
            prefix,
            limit,
            open: RefCell::new(BTreeMap::new()),
            written: None,
        }
    }

    fn path(&self, number: u64) -> PathBuf {
        self.directory.join(format!("{}-{number:06}.dat", self.prefix))
    }

    fn numbers(&self) -> Vec<u64> {
        let Ok(files) = fs::read_dir(&self.directory) else {
            return Vec::new();
        };
        let prefix = format!("{}-", self.prefix);
        let mut numbers: Vec<u64> = files
            .filter_map(Result::ok)
            .filter_map(|file| {
                let name = file.file_name().into_string().ok()?;
                name.strip_prefix(&prefix)?.strip_suffix(".dat")?.parse().ok()
            })
            .collect();
        numbers.sort_unstable();
        numbers
    }

    /// Runs `work` on a segment's file, opening it if it must. `None` when it cannot.
    fn with<T>(&self, number: u64, create: bool, work: impl FnOnce(&File) -> T) -> Option<T> {
        let mut open = self.open.borrow_mut();
        if !open.contains_key(&number) {
            if open.len() >= OPEN_AT_ONCE {
                open.pop_first();
            }
            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .create(create)
                .truncate(false)
                .open(self.path(number))
                .ok()?;
            open.insert(number, file);
        }
        open.get(&number).map(work)
    }

    /// Splits a range at segment ends.
    fn pieces(at: u64, length: usize) -> impl Iterator<Item = (u64, u64, usize, usize)> {
        let mut done = 0usize;
        std::iter::from_fn(move || {
            if done >= length {
                return None;
            }
            let position = at + done as u64;
            let inside = position % SEGMENT;
            let size = ((SEGMENT - inside) as usize).min(length - done);
            let piece = (position / SEGMENT, inside, done, size);
            done += size;
            Some(piece)
        })
    }
}

impl Medium for Segments {
    fn capacity(&self) -> u64 {
        self.limit.unwrap_or(u64::MAX)
    }

    /// What was never written reads as erased, as on flash.
    fn read(&self, at: u64, bytes: &mut [u8]) -> bool {
        bytes.fill(0xFF);
        for (number, inside, start, size) in Self::pieces(at, bytes.len()) {
            let part = &mut bytes[start..start + size];
            let mut filled = 0;
            let done = self.with(number, false, |mut file| {
                if file.seek(SeekFrom::Start(inside)).is_err() {
                    return;
                }
                while filled < part.len() {
                    match file.read(&mut part[filled..]) {
                        Ok(0) | Err(_) => break,
                        Ok(count) => filled += count,
                    }
                }
            });
            if done.is_none() && !self.path(number).exists() {
                let wanted = self.numbers().first().is_none_or(|lowest| number >= *lowest);
                if !wanted {
                    return false;
                }
            }
        }
        true
    }

    fn write(&mut self, at: u64, bytes: &[u8]) -> bool {
        let mut fine = true;
        for (number, inside, start, size) in Self::pieces(at, bytes.len()) {
            let part = &bytes[start..start + size];
            let wrote = self.with(number, true, |mut file| {
                file.seek(SeekFrom::Start(inside)).and_then(|_| file.write_all(part)).is_ok()
            });
            fine &= wrote == Some(true);
            self.written = Some(number);
        }
        fine
    }

    fn erase(&mut self, at: u64) -> bool {
        self.write(at, &[0xFF; SECTOR as usize])
    }

    fn lowest(&self) -> u64 {
        self.numbers().first().map_or(0, |number| number * SEGMENT)
    }

    /// Every write makes its segment's file, so nothing lies past the last one.
    fn written_end(&self) -> Option<u64> {
        Some(self.numbers().last().map_or(0, |number| (number + 1) * SEGMENT))
    }

    fn forget(&mut self, below: u64) {
        for number in self.numbers() {
            if (number + 1) * SEGMENT <= below {
                self.open.borrow_mut().remove(&number);
                let _ = fs::remove_file(self.path(number));
            }
        }
    }

    fn sync(&mut self) {
        if let Some(number) = self.written {
            let _ = self.with(number, false, File::sync_data);
        }
    }
}

/// One file with every header at its height, so a pruned computer can still show them all.
struct HeaderFile(File);

impl Shelf for HeaderFile {
    fn put(&mut self, height: u64, header: &BlockHeader) -> bool {
        let at = height * BlockHeader::ENCODED_LEN as u64;
        let mut file = &self.0;
        file.seek(SeekFrom::Start(at)).and_then(|_| file.write_all(&header.encode())).is_ok()
    }

    fn get(&self, height: u64) -> Option<BlockHeader> {
        let mut bytes = [0u8; BlockHeader::ENCODED_LEN];
        let mut file = &self.0;
        file.seek(SeekFrom::Start(height * BlockHeader::ENCODED_LEN as u64)).ok()?;
        file.read_exact(&mut bytes).ok()?;
        // A height that was never written reads as zeros.
        (bytes != [0u8; BlockHeader::ENCODED_LEN]).then(|| BlockHeader::decode(&bytes))?
    }
}

/// Where a chain keeps its blocks and its saved state in `directory`.
pub fn keep_in(directory: &Path, recent: Option<u64>, limit: Option<u64>) -> Result<Keep, String> {
    fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    let headers = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(directory.join(EVERY_HEADER))
        .map_err(|error| error.to_string())?;
    Ok(Keep {
        log: Box::new(Segments::new(directory, "chain", limit)),
        states: [
            Box::new(Segments::new(directory, "state-a", None)),
            Box::new(Segments::new(directory, "state-b", None)),
        ],
        shelf: Some(Box::new(HeaderFile(headers))),
        recent,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use olc_core::CoinConfig;

    #[test]
    fn the_header_file_holds_each_height_and_knows_its_gaps() {
        let folder = std::env::temp_dir().join(format!("olc-shelf-{}", std::process::id()));
        let mut keep = keep_in(&folder, None, None).unwrap();
        let shelf = keep.shelf.as_mut().unwrap();
        let mut header = CoinConfig::olc().genesis().header;
        assert!(shelf.put(5, &header));
        assert_eq!(shelf.get(5), Some(header));
        assert_eq!(shelf.get(4), None);
        assert_eq!(shelf.get(6), None);
        header.nonce += 1;
        assert!(shelf.put(5, &header));
        assert_eq!(shelf.get(5), Some(header));
        drop(keep);
        let keep = keep_in(&folder, None, None).unwrap();
        assert_eq!(keep.shelf.unwrap().get(5), Some(header));
        fs::remove_dir_all(folder).unwrap();
    }

    fn plugins() -> olc_core::Plugins {
        olc_core::Plugins { consensus: Box::new(olc_pow::ProofOfWork), languages: Vec::new() }
    }

    /// A new miner's first open reads only what was written, not every sector its limit allows:
    /// the 32 GiB a Raspberry Pi gets opens at once.
    #[test]
    fn a_large_empty_limit_opens_at_once() {
        let folder = std::env::temp_dir().join(format!("olc-large-{}", std::process::id()));
        let began = std::time::Instant::now();
        let chain = crate::open_chain(&folder, CoinConfig::olc(), plugins(), Some(32 << 30)).unwrap();
        let took = began.elapsed();
        assert_eq!(chain.height(), 0);
        fs::remove_dir_all(&folder).unwrap();
        assert!(took.as_millis() < 1_000, "opening took {took:?}");
    }

    /// Without its saved state a store finds its blocks by reading what was written.
    #[test]
    fn a_store_without_its_state_finds_its_blocks() {
        let folder = std::env::temp_dir().join(format!("olc-survey-{}", std::process::id()));
        let coin = CoinConfig::olc();
        let mut chain = crate::open_chain(&folder, coin.clone(), plugins(), Some(32 << 30)).unwrap();
        for _ in 0..3 {
            let time_ms = chain.earliest_time(&chain.tip()).max(chain.tip_header().time_ms + 30_000);
            let mut block = chain.block_template(&[7; 20], &[], time_ms);
            assert!(olc_core::Consensus::try_seal(&olc_pow::ProofOfWork, &mut block.header, u32::MAX));
            chain.add_block(block).unwrap();
        }
        let tip = chain.tip();
        drop(chain);
        for state in ["state-a", "state-b"] {
            for number in 0..2 {
                let _ = fs::remove_file(folder.join(format!("{state}-{number:06}.dat")));
            }
        }
        let again = crate::open_chain(&folder, coin, plugins(), Some(32 << 30)).unwrap();
        assert_eq!(again.tip(), tip);
        fs::remove_dir_all(&folder).unwrap();
    }
}
