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

pub(super) fn peers(desk: &mut dyn Desk, _args: &Args) -> Result<String, String> {
    order(desk, ChannelOrder::Peers)
}

pub(super) fn disconnect(desk: &mut dyn Desk, args: &Args) -> Result<String, String> {
    let saying = "Give the connection's number, as olc peers lists it first on its line.";
    let number = word(args, 1, saying)?.parse::<u32>().map_err(|_| saying.to_string())?;
    order(desk, ChannelOrder::Disconnect(number))
}

pub(super) fn ban(desk: &mut dyn Desk, args: &Args) -> Result<String, String> {
    let address = word(args, 1, "Give the IP address to keep out, as olc peers shows it.")?;
    let hours = ban_hours(args.word(2))?;
    order(desk, ChannelOrder::Ban { address, hours })
}

pub(super) fn unban(desk: &mut dyn Desk, args: &Args) -> Result<String, String> {
    let address = word(args, 1, "Give the IP address to let back, as olc peers lists it under Banned.")?;
    order(desk, ChannelOrder::Unban(address))
}

/// How long a ban lasts: an hour unless a day is asked for.
fn ban_hours(typed: Option<&str>) -> Result<u64, String> {
    match typed.map(str::to_lowercase).as_deref() {
        None | Some("1h") => Ok(1),
        Some("1d" | "24h") => Ok(24),
        Some(other) => Err(format!("{other} is not a length for a ban. Say 1h for an hour or 1d for a day.")),
    }
}

#[cfg(test)]
mod tests {
    use super::super::snapshot::BoardView;
    use super::super::words::{run_line, WORDS};
    use super::*;

    /// Keeps the orders a line asks for.
    #[derive(Default)]
    struct Heard(Vec<ChannelOrder>);

    impl Desk for Heard {
        fn command(&mut self, command: Command) -> Result<String, String> {
            if let Command::Channel(order) = command {
                self.0.push(order);
            }
            Ok(String::new())
        }

        fn boards(&mut self) -> Result<Vec<BoardView>, String> {
            Ok(Vec::new())
        }

        fn password(&mut self) -> Result<String, String> {
            Err("no password here".into())
        }
    }

    fn asked(line: &str) -> Result<ChannelOrder, String> {
        let mut desk = Heard::default();
        run_line(&mut desk, line)?;
        desk.0.pop().ok_or_else(|| format!("{line} asked nothing"))
    }

    #[test]
    fn peer_words_reach_the_orders_the_peers_list_sends() {
        assert_eq!(asked("peers"), Ok(ChannelOrder::Peers));
        assert_eq!(asked("disconnect 7"), Ok(ChannelOrder::Disconnect(7)));
        assert!(asked("disconnect seven").is_err());
        assert!(asked("disconnect").is_err());
        let ban = |hours| Ok(ChannelOrder::Ban { address: "192.168.0.5".into(), hours });
        assert_eq!(asked("ban 192.168.0.5"), ban(1));
        assert_eq!(asked("ban 192.168.0.5 1h"), ban(1));
        assert_eq!(asked("ban 192.168.0.5 1d"), ban(24));
        assert!(asked("ban 192.168.0.5 1w").is_err());
        assert_eq!(asked("unban 192.168.0.5"), Ok(ChannelOrder::Unban("192.168.0.5".into())));
    }

    #[test]
    fn a_channel_opens_from_one_pasted_address_with_push_and_delay() {
        let open = asked("channel-open 02ab@127.0.0.1:7000 5 --push 1 --delay 200");
        let wanted = ChannelOrder::Open { peer: "02ab@127.0.0.1:7000".into(), amount: "5".into(), delay: "200".into(), push: "1".into() };
        assert_eq!(open, Ok(wanted));
        let usage = WORDS.iter().find(|word| word.name == "channel-open").map_or("", |word| word.usage);
        assert!(usage.starts_with("channel-open KEY@HOST:PORT AMOUNT [--push AMOUNT] [--delay BLOCKS]"), "{usage}");
    }
}
