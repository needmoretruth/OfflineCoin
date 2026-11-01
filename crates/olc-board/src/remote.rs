//! What a person or a program does to one board of the fleet: waiting for the answer, repeating
//! what got lost. The packets themselves are in `fleet`.
use crate::board::BoardLink;
use crate::fleet::Fleet;
use olc_control::{Action, BoardId, Read, Refusal, CHUNK};
use olc_crypto::{Hash, StreamHash};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const POLL: Duration = Duration::from_millis(5);
const ORDER_TRIES: u32 = 4;
const ORDER_WAIT: Duration = Duration::from_secs(2);
const CHUNK_TRIES: u32 = 10;
const CHUNK_WAIT: Duration = Duration::from_millis(1_200);
/// Reading the whole image back takes the board about this long.
const LAST_CHUNK_WAIT: Duration = Duration::from_secs(5);

pub type SharedFleet = Arc<Mutex<Fleet>>;

fn with<T>(fleet: &SharedFleet, action: impl FnOnce(&mut Fleet) -> T) -> Result<T, String> {
    let mut fleet = fleet.lock().map_err(|_| "The app stopped.".to_string())?;
    Ok(action(&mut fleet))
}

/// Sends a line to a board and returns its answer once it ran. Repeats the same signed order, so
/// a line that ran and lost its answer does not run twice.
pub fn command(fleet: &SharedFleet, board: BoardId, line: &str) -> Result<Option<String>, String> {
    let mut answer = None;
    for _ in 0..ORDER_TRIES {
        let action = Action::Line(line.to_string());
        let signed = with(fleet, |fleet| fleet.sign(board, action))??;
        let counter = signed.order.counter;
        for attempt in 0..ORDER_TRIES {
            let copy = signed.clone();
            with(fleet, |fleet| fleet.send_signed(board, copy))?;
            let deadline = Instant::now() + ORDER_WAIT;
            while Instant::now() < deadline {
                while let Some(line) = with(fleet, |fleet| fleet.take_line(board))? {
                    if !line.starts_with("done ") {
                        answer = Some(line);
                    }
                }
                if with(fleet, |fleet| fleet.has_finished(board, counter))? {
                    return Ok(answer);
                }
                match with(fleet, |fleet| fleet.take_refusal(board))? {
                    // A repeat that is refused means the first copy got there.
                    Some(Refusal::Replayed) if attempt > 0 => return Ok(answer),
                    // The board was ahead of what this computer knew, so sign anew.
                    Some(Refusal::Replayed) => break,
                    Some(reason) => return Err(format!("refused: {reason:?}")),
                    None => {}
                }
                thread::sleep(POLL);
            }
            if Instant::now() < deadline {
                break;
            }
        }
    }
    Err("The board did not answer.".to_string())
}

pub fn hash_of(image: &[u8]) -> Hash {
    let mut hasher = StreamHash::default();
    hasher.update(image);
    hasher.finish()
}

fn new_session() -> Result<u32, String> {
    let mut bytes = [0u8; 4];
    getrandom::fill(&mut bytes).map_err(|error| error.to_string())?;
    Ok(u32::from_le_bytes(bytes))
}

fn wait_for_progress(
    fleet: &SharedFleet,
    board: BoardId,
    session: u32,
    past: Option<u32>,
    wait: Duration,
) -> Result<Option<u32>, String> {
    let deadline = Instant::now() + wait;
    while Instant::now() < deadline {
        if let Some(reason) = with(fleet, |fleet| fleet.take_refusal(board))? {
            return Err(format!("refused: {reason:?}"));
        }
        let next = with(fleet, |fleet| fleet.progress(session))?;
        if next.is_some() && next > past {
            return Ok(next);
        }
        thread::sleep(POLL);
    }
    Ok(None)
}

/// Sends `image` under a signature for `hash`, which a true update takes from the image itself.
/// Returns how long it took. The board restarts on its own once it has checked all of it.
pub fn send_image(
    fleet: &SharedFleet,
    board: BoardId,
    image: &[u8],
    hash: Hash,
    on_progress: &dyn Fn(u32),
) -> Result<Duration, String> {
    let started = Instant::now();
    let session = new_session()?;
    let size = u32::try_from(image.len()).map_err(|_| "The image is too large.")?;
    let mut begun = None;
    for _ in 0..ORDER_TRIES {
        let action = Action::Update {
            session,
            size,
            image: hash,
        };
        with(fleet, |fleet| fleet.order(board, action))??;
        begun = wait_for_progress(fleet, board, session, None, ORDER_WAIT)?;
        if begun.is_some() {
            break;
        }
    }
    let mut next = begun.ok_or("The board did not start the update.")?;
    while next < size {
        let end = (next as usize + CHUNK).min(image.len());
        let data = &image[next as usize..end];
        let wait = if end == image.len() {
            LAST_CHUNK_WAIT
        } else {
            CHUNK_WAIT
        };
        let mut moved = None;
        for _ in 0..CHUNK_TRIES {
            with(fleet, |fleet| {
                fleet.chunk(board, session, next, data.to_vec())
            })?;
            moved = wait_for_progress(fleet, board, session, Some(next), wait)?;
            if moved.is_some() {
                break;
            }
        }
        next = moved.ok_or("The board stopped answering during the update.")?;
        on_progress(next);
    }
    Ok(started.elapsed())
}

/// A board of the fleet behind the same socket a cable uses.
pub struct RemoteBoard {
    fleet: SharedFleet,
    id: BoardId,
}

impl RemoteBoard {
    pub fn new(fleet: SharedFleet, id: BoardId) -> Self {
        Self { fleet, id }
    }
}

fn decode_id(text: &str) -> Option<Hash> {
    let mut id = [0u8; 32];
    if text.len() != 64 || !text.is_ascii() {
        return None;
    }
    for (index, byte) in id.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16).ok()?;
    }
    Some(id)
}

impl BoardLink for RemoteBoard {
    fn send(&mut self, line: &str) -> Result<(), String> {
        let id = self.id;
        let line = line.trim();
        let read = match line.split_once(' ') {
            None if line == "status" => Some(Read::Status),
            Some(("tx", hash)) => decode_id(hash).map(Read::Tx),
            _ => None,
        };
        with(&self.fleet, |fleet| match read {
            Some(read) => {
                fleet.read(id, read);
                Ok(())
            }
            None => fleet.order(id, Action::Line(line.to_string())),
        })?
    }

    fn receive(&mut self, wait: Duration) -> Option<String> {
        let id = self.id;
        let deadline = Instant::now() + wait;
        loop {
            let line = with(&self.fleet, |fleet| fleet.take_line(id)).ok()?;
            if line.is_some() || Instant::now() >= deadline {
                return line;
            }
            thread::sleep(POLL);
        }
    }
}
