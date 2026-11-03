//! `olc swap`: a coin swap from the command line, with the same orders the swap page sends.
use super::commands::{Command, SwapOrder};
use super::words::Desk;
use olc_board::Args;

/// Minutes each side waits before it may take its coins back, unless `--wait` says otherwise.
pub const WAIT_MINUTES: u32 = 60;

const USE: &str = "Use: olc swap offer COIN GIVE TAKE [--wait MINUTES] | accept TEXT | answer TEXT | cancel ID | list";

pub fn swap(desk: &mut dyn Desk, args: &Args) -> Result<String, String> {
    let word = |index: usize| args.word(index).map(str::to_string);
    let order = match args.word(1) {
        Some("offer") => {
            let (Some(coin), Some(give), Some(take)) = (word(2), word(3), word(4)) else { return Err(USE.into()) };
            let wait = args.value("wait").map_or(Ok(WAIT_MINUTES), str::parse).map_err(|_| "Say the wait in minutes.")?;
            SwapOrder::Offer { coin, give, take, wait_minutes: wait }
        }
        Some("accept") => SwapOrder::Accept(word(2).ok_or("Paste the offer text.")?),
        Some("answer") => SwapOrder::Answer(word(2).ok_or("Paste the answer text.")?),
        Some("cancel") => SwapOrder::Cancel(word(2).ok_or("Name the swap, as in olc swap list.")?),
        Some("list") | None => SwapOrder::List,
        Some(_) => return Err(USE.into()),
    };
    desk.command(Command::Swap(order))
}
