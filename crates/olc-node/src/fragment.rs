//! Splits messages for small-frame transports such as ESP-NOW, and joins them again.
use alloc::vec;
use alloc::vec::Vec;

/// network mark (4 bytes), message id (2 bytes), fragment index, fragment count. The mark leads
/// every frame so a receiver can drop one without keeping anything.
pub const HEADER: usize = 8;
const MARK: usize = 4;
const MAX_PARTIALS: usize = 8;
/// A frame holds one byte of the count, so no more pieces than this can name their place.
const MOST_PIECES: usize = 255;
/// Bytes of unfinished messages kept. Anyone in reach can start one and need never finish it, and
/// a board has to spare this much. A coin's largest block should stay under it.
const MOST_HELD: usize = 256 * 1024;

/// `message` starts with its mark, which moves into each frame's header. One too long for the
/// frames gives none: half a message would only be taken for a damaged one.
pub fn split(id: u16, message: &[u8], frame_size: usize) -> Vec<Vec<u8>> {
    let payload = frame_size.saturating_sub(HEADER).max(1);
    let (mark, body) = message.split_at(MARK.min(message.len()));
    let pieces: Vec<&[u8]> = body.chunks(payload).collect();
    if pieces.len() > MOST_PIECES {
        return Vec::new();
    }
    let count = pieces.len() as u8;
    pieces
        .iter()
        .enumerate()
        .map(|(index, piece)| {
            let mut frame = Vec::with_capacity(HEADER + piece.len());
            frame.extend_from_slice(mark);
            frame.extend_from_slice(&id.to_le_bytes());
            frame.push(index as u8);
            frame.push(count);
            frame.extend_from_slice(piece);
            frame
        })
        .collect()
}

struct Partial {
    source: u32,
    id: u16,
    pieces: Vec<Option<Vec<u8>>>,
}

#[derive(Default)]
pub struct Reassembler {
    partials: Vec<Partial>,
}

impl Reassembler {
    /// Returns the whole message once its last fragment has arrived.
    pub fn push(&mut self, source: u32, frame: &[u8]) -> Option<Vec<u8>> {
        if frame.len() < HEADER {
            return None;
        }
        let id = u16::from_le_bytes([frame[MARK], frame[MARK + 1]]);
        let (index, count) = (usize::from(frame[MARK + 2]), usize::from(frame[MARK + 3]));
        if index >= count {
            return None;
        }
        self.make_room(frame.len() - HEADER, (source, id));
        let found = self
            .partials
            .iter()
            .position(|partial| partial.source == source && partial.id == id);
        let slot = match found {
            Some(slot) => slot,
            None => {
                if self.partials.len() >= MAX_PARTIALS {
                    self.partials.remove(0);
                }
                self.partials.push(Partial {
                    source,
                    id,
                    pieces: vec![None; count],
                });
                self.partials.len() - 1
            }
        };
        let partial = &mut self.partials[slot];
        if partial.pieces.len() != count {
            return None;
        }
        partial.pieces[index] = Some(frame[HEADER..].to_vec());
        if partial.pieces.iter().any(Option::is_none) {
            return None;
        }
        let done = self.partials.remove(slot);
        let mut message = frame[..MARK].to_vec();
        message.extend(done.pieces.into_iter().flatten().flatten());
        Some(message)
    }
}

impl Reassembler {
    fn held(&self) -> usize {
        let pieces = self.partials.iter().flat_map(|partial| &partial.pieces);
        pieces.flatten().map(Vec::len).sum()
    }

    /// Gives up the oldest other messages until `size` more bytes fit. A message larger than the
    /// limit goes as well.
    fn make_room(&mut self, size: usize, keep: (u32, u16)) {
        while self.held().saturating_add(size) > MOST_HELD {
            let other = self.partials.iter().position(|one| (one.source, one.id) != keep);
            match other {
                Some(oldest) => self.partials.remove(oldest),
                None => {
                    self.partials.clear();
                    return;
                }
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The largest frames, in the largest messages, none of them ever finished.
    #[test]
    fn unfinished_messages_keep_to_a_limit() {
        let mut joiner = Reassembler::default();
        for id in 0..1_000u16 {
            for index in 0..254u8 {
                let mut frame = alloc::vec![0xAA; 1_470];
                frame[MARK..MARK + 2].copy_from_slice(&id.to_le_bytes());
                frame[MARK + 2] = index;
                frame[MARK + 3] = 255;
                assert!(joiner.push(u32::from(id), &frame).is_none());
                assert!(joiner.held() <= MOST_HELD, "{} bytes held", joiner.held());
            }
        }
    }

    #[test]
    fn a_message_too_long_for_its_frames_gives_none() {
        assert!(split(1, &[0u8; 300_000], 250).is_empty());
        assert_eq!(split(1, &[0u8; 300_000], 1_470).len(), 206);
    }

    #[test]
    fn out_of_order_fragments_join() {
        let message: Vec<u8> = (0..=255u8).cycle().take(1_000).collect();
        let mut frames = split(7, &message, 250);
        assert!(frames.iter().all(|frame| frame.len() <= 250));
        frames.reverse();
        let mut joiner = Reassembler::default();
        let mut result = None;
        for frame in &frames {
            result = joiner.push(1, frame).or(result);
        }
        assert_eq!(result, Some(message));
    }

    #[test]
    fn a_missing_fragment_yields_nothing() {
        let frames = split(1, &[0u8; 600], 250);
        let mut joiner = Reassembler::default();
        assert!(frames[1..]
            .iter()
            .all(|frame| joiner.push(1, frame).is_none()));
    }
}
