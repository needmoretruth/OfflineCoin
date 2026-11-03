//! The words for payment channels. Each ends in the same [`ChannelOrder`] the channels page sends.
use super::commands::{ChannelOrder, Command};
use super::words::Desk;
use olc_board::Args;

fn order(desk: &mut dyn Desk, order: ChannelOrder) -> Result<String, String> {
    desk.command(Command::Channel(order))
}

fn word(args: &Args, index: usize, saying: &str) -> Result<String, String> {
    args.word(index).map(str::to_string).ok_or_else(|| saying.to_string())
}

pub(super) fn channels(desk: &mut dyn Desk, _args: &Args) -> Result<String, String> {
    order(desk, ChannelOrder::Show)
}

pub(super) fn channel_open(desk: &mut dyn Desk, args: &Args) -> Result<String, String> {
    let peer = word(args, 1, "Give the other node's channel address, KEY@HOST:PORT, as it shows on its Channels page.")?;
    let amount = word(args, 2, "Say how much to put in the channel.")?;
    let delay = args.value("delay").unwrap_or_default().to_string();
    let push = args.value("push").unwrap_or_default().to_string();
    order(desk, ChannelOrder::Open { peer, amount, delay, push })
}

pub(super) fn channel_close(desk: &mut dyn Desk, args: &Args) -> Result<String, String> {
    let channel = word(args, 1, "Name the channel, as olc channels lists it.")?;
    order(desk, ChannelOrder::Close { channel, force: args.has("force") })
}

pub(super) fn invoice(desk: &mut dyn Desk, args: &Args) -> Result<String, String> {
    let amount = word(args, 1, "Say how much to ask for.")?;
    let note = (2..).map_while(|index| args.word(index)).collect::<Vec<_>>().join(" ");
    order(desk, ChannelOrder::Invoice { amount, note })
}

pub(super) fn channel_pay(desk: &mut dyn Desk, args: &Args) -> Result<String, String> {
    let text = word(args, 1, "Give the payment request.")?;
    order(desk, ChannelOrder::Pay { text })
}
