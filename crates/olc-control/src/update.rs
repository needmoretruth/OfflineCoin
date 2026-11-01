//! Receives a firmware image into the slot that is not running. Nothing here touches the running
//! build: the board switches only after the whole image, read back, matches the signed hash.
use crate::packet::Refusal;
use alloc::vec;
use alloc::vec::Vec;
use olc_crypto::{Hash, StreamHash};

/// Image bytes per packet, which fits a handful of radio frames.
pub const CHUNK: usize = 1024;
/// Flash erases this much at once, so writes wait for a whole page.
pub const PAGE: usize = 4096;

/// The flash slot the image goes to.
pub trait ImageSlot {
    fn capacity(&self) -> u32;
    fn write_page(&mut self, offset: u32, page: &[u8]) -> bool;
    fn read(&mut self, offset: u32, bytes: &mut [u8]) -> bool;
}

pub struct Update {
    session: u32,
    size: u32,
    image: Hash,
    received: u32,
    page: Vec<u8>,
}

impl Update {
    pub fn begin(session: u32, size: u32, image: Hash, capacity: u32) -> Result<Self, Refusal> {
        if size == 0 || size > capacity {
            return Err(Refusal::TooLarge);
        }
        Ok(Self {
            session,
            size,
            image,
            received: 0,
            page: Vec::with_capacity(PAGE),
        })
    }

    pub fn session(&self) -> u32 {
        self.session
    }

    /// Where the next chunk starts. A chunk that starts anywhere else is ignored, so a lost or
    /// repeated one costs only a resend.
    pub fn next(&self) -> u32 {
        self.received
    }

    pub fn done(&self) -> bool {
        self.received == self.size
    }

    pub fn chunk(
        &mut self,
        slot: &mut dyn ImageSlot,
        offset: u32,
        data: &[u8],
    ) -> Result<(), Refusal> {
        let fits = self.received as usize + data.len() <= self.size as usize;
        if offset != self.received || data.is_empty() || !fits {
            return Ok(());
        }
        let mut rest = data;
        while !rest.is_empty() {
            let take = rest.len().min(PAGE - self.page.len());
            self.page.extend_from_slice(&rest[..take]);
            rest = &rest[take..];
            self.received += take as u32;
            if self.page.len() == PAGE || self.done() {
                let start = (self.received - 1) / PAGE as u32 * PAGE as u32;
                if !slot.write_page(start, &self.page) {
                    return Err(Refusal::FlashFailed);
                }
                self.page.clear();
            }
        }
        Ok(())
    }

    /// Reads the slot back. Only a true answer may switch the board to the new image.
    pub fn verify(&self, slot: &mut dyn ImageSlot) -> Result<(), Refusal> {
        let mut hasher = StreamHash::default();
        let mut buffer = vec![0u8; PAGE];
        let mut offset = 0;
        while offset < self.size {
            let size = (self.size - offset).min(PAGE as u32) as usize;
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

    fn send_all(update: &mut Update, slot: &mut Slot, image: &[u8]) {
        for (index, chunk) in image.chunks(CHUNK).enumerate() {
            update
                .chunk(slot, (index * CHUNK) as u32, chunk)
                .expect("flash works");
        }
    }

    #[test]
    fn a_whole_image_lands_and_verifies() {
        let bytes = image(10_500);
        let mut slot = Slot::new(20_000);
        let mut update = Update::begin(1, 10_500, hash_of(&bytes), slot.capacity()).unwrap();
        send_all(&mut update, &mut slot, &bytes);
        assert!(update.done());
        assert_eq!(slot.writes, vec![0, 4096, 8192]);
        assert_eq!(update.verify(&mut slot), Ok(()));
        assert_eq!(&slot.bytes[..10_500], &bytes[..]);
    }

    #[test]
    fn a_flipped_byte_is_found_after_all_of_it_arrived() {
        let bytes = image(5_000);
        let mut slot = Slot::new(8_192);
        let mut update = Update::begin(1, 5_000, hash_of(&bytes), slot.capacity()).unwrap();
        let mut bad = bytes.clone();
        bad[4_321] ^= 1;
        send_all(&mut update, &mut slot, &bad);
        assert!(update.done());
        assert_eq!(update.verify(&mut slot), Err(Refusal::BadImage));
    }

    #[test]
    fn a_repeated_or_skipped_chunk_changes_nothing() {
        let bytes = image(3_000);
        let mut slot = Slot::new(4_096);
        let mut update = Update::begin(1, 3_000, hash_of(&bytes), slot.capacity()).unwrap();
        let mut sink = Slot::new(4_096);
        update.chunk(&mut sink, 0, &bytes[..CHUNK]).unwrap();
        update.chunk(&mut sink, 0, &bytes[..CHUNK]).unwrap();
        update.chunk(&mut sink, 2_048, &bytes[2_048..]).unwrap();
        assert_eq!(update.next(), CHUNK as u32);
        update
            .chunk(&mut sink, 1_024, &bytes[1_024..2_048])
            .unwrap();
        update.chunk(&mut sink, 2_048, &bytes[2_048..]).unwrap();
        assert!(update.done());
        assert_eq!(update.verify(&mut sink), Ok(()));
        assert!(slot.writes.is_empty());
        slot.broken = false;
    }

    #[test]
    fn an_image_too_big_for_the_slot_is_refused() {
        assert_eq!(
            Update::begin(1, 9_000, [0; 32], 8_192).err(),
            Some(Refusal::TooLarge)
        );
    }

    #[test]
    fn a_failed_write_stops_the_update() {
        let bytes = image(4_096);
        let mut slot = Slot::new(8_192);
        slot.broken = true;
        let mut update = Update::begin(1, 4_096, hash_of(&bytes), slot.capacity()).unwrap();
        let result = bytes
            .chunks(CHUNK)
            .enumerate()
            .try_for_each(|(index, chunk)| update.chunk(&mut slot, (index * CHUNK) as u32, chunk));
        assert_eq!(result, Err(Refusal::FlashFailed));
    }
}
