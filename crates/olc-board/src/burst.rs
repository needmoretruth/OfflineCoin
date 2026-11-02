//! Sends a firmware image in bursts: one signed order, then pieces with no answer each, then a
//! question about what is missing. The question travels behind the pieces, so its answer also
//! says the board has caught up.
use crate::clearance::Cleared;
use crate::fleet::Connection;
use crate::remote::{new_session, with, SharedFleet};
use olc_control::{Action, BoardId, Body, Refusal, PIECE};
use std::thread;
use std::time::{Duration, Instant};

const POLL: Duration = Duration::from_millis(2);
/// Pieces between two questions. A board keeps 8 KB of a cable, which is about this much.
const WINDOW: u32 = 2;
const ORDER_TRIES: u32 = 3;
/// A signature takes the board a moment at its lowest speed.
const ORDER_WAIT: Duration = Duration::from_secs(2);
const ASK_WAIT: Duration = Duration::from_millis(1_500);
/// A board reads its whole image to check it before it sends.
const HUB_WAIT: Duration = Duration::from_secs(12);
const FOLLOW_EVERY: Duration = Duration::from_millis(500);
/// Writing a page of flash or changing branch can hold a board this long.
const STALLED_AFTER: Duration = Duration::from_secs(30);
/// The board restarts by itself as soon as it has all of it, sometimes between two questions.
const SILENT_AT_THE_END: Duration = Duration::from_secs(6);

type Word = (u32, Vec<(u32, u32)>);

/// False when the board never answered the order, as a build from before bursts does not.
pub(crate) fn send(
    fleet: &SharedFleet,
    cleared: &Cleared,
    image: &[u8],
    on_progress: &dyn Fn(u32),
) -> Result<bool, String> {
    let board = cleared.board;
    let session = new_session()?;
    let size = u32::try_from(image.len()).map_err(|_| "The image is too large.".to_string())?;
    let mut word = None;
    for _ in 0..ORDER_TRIES {
        let action = Action::Burst {
            session,
            size,
            image: cleared.hash,
        };
        with(fleet, |fleet| fleet.order(board, action))??;
        word = wait_for_word(fleet, board, session, ORDER_WAIT)?;
        if word.is_some() {
            break;
        }
    }
    let Some(mut word) = word else {
        return Ok(false);
    };
    if let Some(hub) = hub_of(fleet, board)? {
        if hub_serves(fleet, hub, cleared, session, size)? {
            follow(fleet, board, session, size, on_progress)?;
            return Ok(true);
        }
    }
    let mut heard = Instant::now();
    loop {
        let (done, runs) = &word;
        on_progress(done / 2);
        if *done >= size.saturating_mul(2) {
            return Ok(true);
        }
        for index in wanted(runs).take(WINDOW as usize) {
            let start = index as usize * PIECE;
            let end = (start + PIECE).min(image.len());
            let body = Body::Piece {
                session,
                index,
                data: image[start..end].to_vec(),
            };
            with(fleet, |fleet| fleet.send_body(board, body))?;
        }
        with(fleet, |fleet| {
            fleet.send_body(board, Body::AskMissing { session })
        })?;
        match wait_for_word(fleet, board, session, ASK_WAIT)? {
            Some(next) => {
                word = next;
                heard = Instant::now();
            }
            None if heard.elapsed() > STALLED_AFTER => {
                return Err("The board stopped answering during the update.".to_string());
            }
            None => {}
        }
    }
}

/// The board with a cable that this one is reached through.
fn hub_of(fleet: &SharedFleet, board: BoardId) -> Result<Option<BoardId>, String> {
    let boards = with(fleet, |fleet| fleet.boards())?;
    let seen = boards.iter().find(|seen| seen.id == board);
    Ok(seen.and_then(|seen| match seen.connection {
        Connection::Through { board } => board,
        Connection::Direct { .. } => None,
    }))
}

/// True when the hub took the job. It refuses when it runs another build, and then the
/// computer sends the image itself.
fn hub_serves(
    fleet: &SharedFleet,
    hub: BoardId,
    cleared: &Cleared,
    session: u32,
    size: u32,
) -> Result<bool, String> {
    for _ in 0..ORDER_TRIES {
        let action = Action::Serve {
            target: cleared.board,
            session,
            size,
            image: cleared.hash,
        };
        with(fleet, |fleet| fleet.order(hub, action))??;
        let deadline = Instant::now() + HUB_WAIT;
        while Instant::now() < deadline {
            match with(fleet, |fleet| fleet.take_refusal(hub))? {
                Some(Refusal::Replayed) | None => {}
                Some(_) => return Ok(false),
            }
            if with(fleet, |fleet| fleet.progress(session))?.is_some() {
                return Ok(true);
            }
            thread::sleep(POLL);
        }
    }
    Ok(false)
}

/// Past nineteen parts in twenty of `done`, which counts to twice the size. The caller still
/// waits for the board to return with the new build, so this only moves the verdict there.
fn nearly_all(done: u32, size: u32) -> bool {
    u64::from(done) * 20 >= u64::from(size) * 2 * 19
}

/// Watches a transfer between two boards until the target has everything in flash.
fn follow(
    fleet: &SharedFleet,
    board: BoardId,
    session: u32,
    size: u32,
    on_progress: &dyn Fn(u32),
) -> Result<(), String> {
    let mut heard = Instant::now();
    let mut last = 0;
    while heard.elapsed() < STALLED_AFTER {
        if nearly_all(last, size) && heard.elapsed() > SILENT_AT_THE_END {
            return Ok(());
        }
        with(fleet, |fleet| {
            fleet.send_body(board, Body::AskMissing { session })
        })?;
        if let Some((done, _)) = wait_for_word(fleet, board, session, FOLLOW_EVERY)? {
            heard = Instant::now();
            last = done;
            on_progress(done / 2);
            if done >= size.saturating_mul(2) {
                return Ok(());
            }
            thread::sleep(FOLLOW_EVERY);
        }
    }
    Err("The board stopped answering during the update.".to_string())
}

fn wanted(runs: &[(u32, u32)]) -> impl Iterator<Item = u32> + '_ {
    runs.iter().flat_map(|(first, count)| *first..first + count)
}

fn wait_for_word(
    fleet: &SharedFleet,
    board: BoardId,
    session: u32,
    wait: Duration,
) -> Result<Option<Word>, String> {
    let deadline = Instant::now() + wait;
    while Instant::now() < deadline {
        match with(fleet, |fleet| fleet.take_refusal(board))? {
            // A copy of an earlier try came in late. The next try has a fresh counter.
            Some(Refusal::Replayed) | None => {}
            Some(reason) => return Err(format!("refused: {reason:?}")),
        }
        let word = with(fleet, |fleet| fleet.take_missing(session))?;
        if word.is_some() {
            return Ok(word);
        }
        thread::sleep(POLL);
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_piece_of_every_stretch_is_wanted_in_order() {
        let pieces: Vec<u32> = wanted(&[(2, 2), (7, 1)]).collect();
        assert_eq!(pieces, [2, 3, 7]);
    }

    #[test]
    fn only_silence_at_the_very_end_counts_as_a_restart() {
        const SIZE: u32 = 2_000_000;
        assert!(!nearly_all(0, SIZE));
        assert!(!nearly_all(SIZE, SIZE));
        assert!(nearly_all(SIZE * 2 / 20 * 19, SIZE));
        assert!(nearly_all(SIZE * 2, SIZE));
    }
}
