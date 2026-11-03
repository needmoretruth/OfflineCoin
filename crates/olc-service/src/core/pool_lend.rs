//! Pool work for a board heard only through the board on one of this app's cables. The fleet signs
//! the work and sends it to that board, which passes it on; the shares come back to the fleet.
//! See `olc_control::lend`.
use super::pool_run::PoolCx;
use super::pool_view::{relay_of, short_address};
use olc_control::BoardId;
use olc_core::BlockHeader;
use olc_wallet::format_address;

/// The board on this app's cable that passes work on to the board that mines to `worker`.
pub fn relay_for(worker: &[u8; 20], cx: &PoolCx) -> Option<BoardId> {
    let fleet = cx.fleet?;
    let (_, link) = cx.hellos.iter().find(|(hello, _)| hello.address == *worker)?;
    relay_of(&fleet.board_rows(), *link).map(|row| row.seen.id)
}

/// What the board calls itself, or its address when it gives no name.
pub fn lent_name(worker: &[u8; 20], hellos: &[(&olc_node::Hello, Option<usize>)]) -> String {
    let hello = hellos.iter().find(|(hello, _)| hello.address == *worker);
    let name = hello.map(|(hello, _)| hello.card.name.clone()).unwrap_or_default();
    match name.is_empty() {
        true => short_address(&format_address(worker)),
        false => name,
    }
}

/// Hands the board work, or stops it with `None`. Says whether the work went out.
pub fn lend(worker: [u8; 20], work: Option<(&BlockHeader, u64)>, cx: &PoolCx) -> bool {
    let (Some(fleet), Some(relay)) = (cx.fleet, relay_for(&worker, cx)) else {
        return false;
    };
    fleet.lend(relay, worker, work.map(|(header, difficulty)| (header.encode().to_vec(), difficulty)))
}

/// The shares the board sent back since the last call.
pub fn take_lent(worker: &[u8; 20], cx: &PoolCx) -> Vec<BlockHeader> {
    let Some(fleet) = cx.fleet else {
        return Vec::new();
    };
    let shares = fleet.take_lent_shares(worker);
    shares.iter().filter_map(|bytes| BlockHeader::decode(bytes)).collect()
}
