//! Putting boards back as they were: after a run, and after a run that died.
use super::{Plan, RESTORE_TRIES};
use olc_board::{Board, Say, Status};

/// A run that died left this board changed, so it starts from its true state.
pub(super) fn put_back_leftovers(board: &mut Board, plan: &Plan, say: &Say) -> Result<(), String> {
    let address = board.status()?.address;
    let left = plan.journal.pending();
    let Some(entry) = left.iter().find(|entry| entry.address == address) else {
        return Ok(());
    };
    entry.lines.iter().try_for_each(|line| board.send(line))?;
    plan.journal.forget(&address);
    say("A run that died left this board changed. It is back as it was.".to_string());
    Ok(())
}

pub(super) fn restore(board: &mut Board, saved: &Status) -> Result<(), String> {
    let mut last = Err("The board did not answer.".to_string());
    for _ in 0..RESTORE_TRIES {
        let sent = saved
            .restore_lines()
            .iter()
            .try_for_each(|line| board.send(line));
        last = sent.and_then(|()| {
            // Two reads: the first may have been printed before the commands ran.
            board.status()?;
            board.status()
        });
        if let Ok(now) = &last {
            if (now.offline, now.limit, now.mining) == (saved.offline, saved.limit, saved.mining) {
                return Ok(());
            }
        }
    }
    last.and(Err(
        "A board did not go back to its saved state.".to_string()
    ))
}
