//! Receives a firmware image in bursts. Pieces land in memory in any order and the board lists
//! what is missing, so a lost one costs one resend instead of a round trip for every piece. Flash
//! is touched only after the whole image matches the signed hash, and the board switches only
//! after the slot, read back, matches it too.
use crate::packet::Refusal;
use alloc::vec;
use alloc::vec::Vec;
use olc_crypto::{Hash, StreamHash};

/// Image bytes per packet for builds from before bursts.
pub const CHUNK: usize = 1024;
/// Image bytes per piece. A piece with its headers fills one large radio frame.
pub const PIECE: usize = 1400;
/// Flash erases this much at once, so writes come in whole pages.
pub const PAGE: usize = 4096;
/// Stretches of missing pieces in one answer. They fit a small radio frame.
const MAX_RUNS: usize = 16;

/// The flash slot the image goes to.
pub trait ImageSlot {
    fn capacity(&self) -> u32;
    fn write_page(&mut self, offset: u32, page: &[u8]) -> bool;
    fn read(&mut self, offset: u32, bytes: &mut [u8]) -> bool;
}

pub struct Update {
    session: u32,
    image: Hash,
    bytes: Vec<u8>,
    have: Vec<u64>,
    pieces_here: u32,
    /// Whether the bytes in memory matched the signed hash.
    checked: bool,
    written: u32,
}

impl Update {
    pub fn begin(session: u32, size: u32, image: Hash, capacity: u32) -> Result<Self, Refusal> {
        if size == 0 || size > capacity {
            return Err(Refusal::TooLarge);
        }
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(size as usize)
            .map_err(|_| Refusal::TooLarge)?;
        bytes.resize(size as usize, 0);
        Ok(Self {
            session,
            image,
            have: vec![0; pieces_of(size).div_ceil(64) as usize],
            bytes,
            pieces_here: 0,
            checked: false,
            written: 0,
        })
    }

    pub fn session(&self) -> u32 {
        self.session
    }

    /// True when a new order asks for the image already arriving, which then goes on from here.
    pub fn same_image(&self, size: u32, image: &Hash) -> bool {
        self.size() == size && self.image == *image
    }

    pub fn go_on_as(&mut self, session: u32) {
        self.session = session;
    }

    fn size(&self) -> u32 {
        self.bytes.len() as u32
    }

    fn holds(&self, index: u32) -> bool {
        self.have[index as usize / 64] >> (index % 64) & 1 == 1
    }

    /// Takes one piece. A repeated or wrong-sized one changes nothing.
    pub fn piece(&mut self, index: u32, data: &[u8]) {
        if index >= pieces_of(self.size()) || self.holds(index) || self.checked {
            return;
        }
        let start = index as usize * PIECE;
        if data.len() != PIECE.min(self.bytes.len() - start) {
            return;
        }
        self.bytes[start..start + data.len()].copy_from_slice(data);
        self.have[index as usize / 64] |= 1 << (index % 64);
        self.pieces_here += 1;
    }

    /// The first stretches still missing, as (first piece, how many).
    pub fn missing(&self) -> Vec<(u32, u32)> {
        let mut runs: Vec<(u32, u32)> = Vec::new();
        for index in 0..pieces_of(self.size()) {
            if self.holds(index) {
                continue;
            }
            if let Some((first, count)) = runs.last_mut() {
                if *first + *count == index {
                    *count += 1;
                    continue;
                }
            }
            if runs.len() == MAX_RUNS {
                break;
            }
            runs.push((index, 1));
        }
        runs
    }

    pub fn all_here(&self) -> bool {
        self.pieces_here == pieces_of(self.size())
    }

    /// Bytes arrived plus bytes written, so twice the size means ready to switch.
    pub fn done(&self) -> u32 {
        let here = (self.pieces_here * PIECE as u32).min(self.size());
        here.saturating_add(self.written)
    }

    pub fn percent(&self) -> u8 {
        (u64::from(self.done()) * 50 / u64::from(self.size().max(1))) as u8
    }

    /// Writes one page, after checking the hash once. True when the last page is in.
    pub fn write_next(&mut self, slot: &mut dyn ImageSlot) -> Result<bool, Refusal> {
        if !self.checked {
            let mut hasher = StreamHash::default();
            hasher.update(&self.bytes);
            if hasher.finish() != self.image {
                return Err(Refusal::BadImage);
            }
            self.checked = true;
        }
        let start = self.written as usize;
        let end = (start + PAGE).min(self.bytes.len());
        if !slot.write_page(self.written, &self.bytes[start..end]) {
            return Err(Refusal::FlashFailed);
        }
        self.written = end as u32;
        Ok(end == self.bytes.len())
    }

    /// Reads the slot back. Only a true answer may switch the board to the new image.
    pub fn verify(&self, slot: &mut dyn ImageSlot) -> Result<(), Refusal> {
        let mut hasher = StreamHash::default();
        let mut buffer = vec![0u8; PAGE];
        let mut offset = 0;
        while offset < self.size() {
            let size = (self.size() - offset).min(PAGE as u32) as usize;
            if !slot.read(offset, &mut buffer[..size]) {
                return Err(Refusal::FlashFailed);
            }
            hasher.update(&buffer[..size]);
            offset += size as u32;
        }
        if hasher.finish() == self.image {
            Ok(())
        } else {
            Err(Refusal::BadImage)
        }
    }
}

fn pieces_of(size: u32) -> u32 {
    size.div_ceil(PIECE as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Slot {
        bytes: Vec<u8>,
        writes: Vec<u32>,
        broken: bool,
    }

    impl Slot {
        fn new(size: usize) -> Self {
            Self {
                bytes: vec![0xFF; size],
                writes: Vec::new(),
                broken: false,
            }
        }
    }

    impl ImageSlot for Slot {
        fn capacity(&self) -> u32 {
            self.bytes.len() as u32
        }

        fn write_page(&mut self, offset: u32, page: &[u8]) -> bool {
            self.writes.push(offset);
            let start = offset as usize;
            self.bytes[start..start + page.len()].copy_from_slice(page);
            !self.broken
        }

        fn read(&mut self, offset: u32, bytes: &mut [u8]) -> bool {
            let start = offset as usize;
            bytes.copy_from_slice(&self.bytes[start..start + bytes.len()]);
            true
        }
    }

    fn image(size: usize) -> Vec<u8> {
        (0..size).map(|index| (index * 7 % 251) as u8).collect()
    }

    fn hash_of(image: &[u8]) -> Hash {
        let mut hasher = StreamHash::default();
        hasher.update(image);
        hasher.finish()
    }

    fn send_all(update: &mut Update, image: &[u8]) {
        for (index, piece) in image.chunks(PIECE).enumerate() {
            update.piece(index as u32, piece);
        }
    }

    fn write_all(update: &mut Update, slot: &mut Slot) -> Result<(), Refusal> {
        while !update.write_next(slot)? {}
        Ok(())
    }

    fn started(image: &[u8]) -> Update {
        Update::begin(1, image.len() as u32, hash_of(image), 1 << 20).unwrap()
    }

    #[test]
    fn a_whole_image_lands_and_verifies() {
        let bytes = image(9_000);
        let mut slot = Slot::new(16_384);
        let mut update = started(&bytes);
        send_all(&mut update, &bytes);
        assert!(update.all_here());
        write_all(&mut update, &mut slot).unwrap();
        update.verify(&mut slot).unwrap();
        assert_eq!(&slot.bytes[..bytes.len()], &bytes[..]);
        assert_eq!(update.percent(), 100);
    }

    #[test]
    fn pieces_in_any_order_land_and_only_the_lost_are_asked_for() {
        let bytes = image(PIECE * 10);
        let mut update = started(&bytes);
        for index in [9u32, 0, 2, 3, 8, 1] {
            let start = index as usize * PIECE;
            update.piece(index, &bytes[start..start + PIECE]);
        }
        assert_eq!(update.missing(), vec![(4, 4)]);
        assert!(!update.all_here());
        for index in 4u32..8 {
            let start = index as usize * PIECE;
            update.piece(index, &bytes[start..start + PIECE]);
        }
        assert!(update.missing().is_empty());
        assert!(update.all_here());
    }

    #[test]
    fn a_changed_piece_never_reaches_flash() {
        let bytes = image(9_000);
        let mut changed = bytes.clone();
        changed[5_000] ^= 1;
        let mut slot = Slot::new(16_384);
        let mut update = started(&bytes);
        send_all(&mut update, &changed);
        assert_eq!(write_all(&mut update, &mut slot), Err(Refusal::BadImage));
        assert!(slot.writes.is_empty());
    }

    #[test]
    fn a_bad_read_back_stops_the_switch() {
        let bytes = image(9_000);
        let mut slot = Slot::new(16_384);
        let mut update = started(&bytes);
        send_all(&mut update, &bytes);
        write_all(&mut update, &mut slot).unwrap();
        slot.bytes[100] ^= 1;
        assert_eq!(update.verify(&mut slot), Err(Refusal::BadImage));
    }

    #[test]
    fn a_new_order_for_the_same_image_goes_on_from_here() {
        let bytes = image(PIECE * 4);
        let mut update = started(&bytes);
        update.piece(0, &bytes[..PIECE]);
        assert!(update.same_image(bytes.len() as u32, &hash_of(&bytes)));
        update.go_on_as(2);
        assert_eq!(update.session(), 2);
        assert_eq!(update.missing(), vec![(1, 3)]);
        assert!(!update.same_image(bytes.len() as u32, &[0; 32]));
    }

    #[test]
    fn a_repeated_or_misshapen_piece_changes_nothing() {
        let bytes = image(PIECE * 2);
        let mut update = started(&bytes);
        update.piece(0, &bytes[..PIECE]);
        update.piece(0, &[0; PIECE]);
        update.piece(1, &bytes[..10]);
        update.piece(7, &bytes[..PIECE]);
        assert_eq!(update.done(), PIECE as u32);
        assert_eq!(update.missing(), vec![(1, 1)]);
    }

    #[test]
    fn an_image_too_big_for_the_slot_is_refused() {
        let begun = Update::begin(1, 100, [0; 32], 99);
        assert!(matches!(begun, Err(Refusal::TooLarge)));
    }

    #[test]
    fn a_failed_write_stops_the_update() {
        let bytes = image(9_000);
        let mut slot = Slot::new(16_384);
        slot.broken = true;
        let mut update = started(&bytes);
        send_all(&mut update, &bytes);
        assert_eq!(write_all(&mut update, &mut slot), Err(Refusal::FlashFailed));
    }
}
