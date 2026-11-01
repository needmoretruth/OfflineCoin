//! A record of how each start of a board went, so a board with no cable can tell afterwards.
//!
//! One word of flash per start. Moving to a later stage only clears bits, so a start costs no
//! erase and a board that stalls leaves the stage it reached.
use alloc::string::String;
use alloc::vec::Vec;

pub const WORDS: usize = 1024;
pub const FREE: u32 = u32::MAX;
const MARK: u32 = 0xB007 << 16;
const MARK_MASK: u32 = 0xFFFF << 16;

/// How far a start got, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Stage {
    Started = 0,
    Radio = 1,
    ChainRead = 2,
    ChainLoaded = 3,
    Running = 4,
}

impl Stage {
    fn from_cleared(bits: u32) -> Self {
        match bits {
            0 => Self::Started,
            1 => Self::Radio,
            2 => Self::ChainRead,
            3 => Self::ChainLoaded,
            _ => Self::Running,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Started => "started",
            Self::Radio => "radio",
            Self::ChainRead => "chain-read",
            Self::ChainLoaded => "chain-loaded",
            Self::Running => "running",
        }
    }
}

/// The word for a start that came from `reason` and reached `stage`.
pub fn word(reason: u8, stage: Stage) -> u32 {
    MARK | u32::from(reason) << 8 | (0xFF << stage as u32) & 0xFF
}

pub fn reason_of(word: u32) -> Option<u8> {
    (word & MARK_MASK == MARK).then_some((word >> 8) as u8)
}

pub fn stage_of(word: u32) -> Stage {
    Stage::from_cleared((word & 0xFF).count_zeros() - 24)
}

/// The chip's own reset codes.
pub fn reason_name(code: u8) -> &'static str {
    match code {
        0x01 => "power-on",
        0x03 | 0x0C => "software",
        0x05 => "deep-sleep",
        0x07 | 0x08 | 0x0B | 0x11 => "timer-watchdog",
        0x09 | 0x0D | 0x10 | 0x12 => "rtc-watchdog",
        0x0F => "brown-out",
        0x13 | 0x17 => "power-glitch",
        0x14 => "efuse",
        0x15 | 0x16 => "usb",
        _ => "unknown",
    }
}

/// Where the next start goes. `None` when every word is used and the sector must be erased.
pub fn next_free(words: &[u32]) -> Option<usize> {
    words.iter().position(|word| *word == FREE)
}

/// The newest starts first, as `reason/stage` words.
pub fn recent(words: &[u32], count: usize) -> Vec<String> {
    words
        .iter()
        .take_while(|word| **word != FREE)
        .filter_map(|word| reason_of(*word).map(|reason| (reason, stage_of(*word))))
        .map(|(reason, stage)| alloc::format!("{}/{}", reason_name(reason), stage.name()))
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .take(count)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_later_stage_only_clears_bits() {
        let stages = [
            Stage::Started,
            Stage::Radio,
            Stage::ChainRead,
            Stage::ChainLoaded,
            Stage::Running,
        ];
        for pair in stages.windows(2) {
            let (before, after) = (word(0x0F, pair[0]), word(0x0F, pair[1]));
            assert_eq!(before & after, after);
            assert_ne!(before, after);
        }
    }

    #[test]
    fn a_word_reads_back() {
        let saved = word(0x0F, Stage::ChainRead);
        assert_eq!(reason_of(saved), Some(0x0F));
        assert_eq!(stage_of(saved), Stage::ChainRead);
        assert_eq!(reason_name(0x0F), "brown-out");
    }

    #[test]
    fn erased_flash_is_no_start() {
        assert_eq!(reason_of(FREE), None);
        assert_eq!(next_free(&[FREE; 4]), Some(0));
        assert!(recent(&[FREE; 4], 3).is_empty());
    }

    #[test]
    fn the_newest_start_comes_first() {
        let words = [
            word(0x01, Stage::Running),
            word(0x0F, Stage::Radio),
            FREE,
            FREE,
        ];
        assert_eq!(next_free(&words), Some(2));
        assert_eq!(recent(&words, 1), ["brown-out/radio"]);
        assert_eq!(recent(&words, 5).len(), 2);
        assert_eq!(next_free(&[word(1, Stage::Started); 2]), None);
    }
}
