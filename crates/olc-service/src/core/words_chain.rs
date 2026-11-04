//! The words a Bitcoin Core user types first at a node: what waits for a block (getrawmempool),
//! this wallet's coins (listunspent), and blocks made now (generatetoaddress).
use super::commands::Command;
use super::making::{ChainOrder, MOST_TO_MAKE};
use super::words::Desk;
use olc_board::Args;

fn order(desk: &mut dyn Desk, order: ChainOrder) -> Result<String, String> {
    desk.command(Command::Chain(order))
}

pub(super) fn mempool(desk: &mut dyn Desk, _args: &Args) -> Result<String, String> {
    order(desk, ChainOrder::Waiting)
}

pub(super) fn coins(desk: &mut dyn Desk, _args: &Args) -> Result<String, String> {
    order(desk, ChainOrder::Coins)
}

/// `generate N` starts, `generate` says how far it is, `generate stop` ends it.
pub(super) fn generate(desk: &mut dyn Desk, args: &Args) -> Result<String, String> {
    let asked = match args.word(1) {
        None => ChainOrder::MakeShow,
        Some("stop") => ChainOrder::MakeStop,
        Some(word) => match word.replace(',', "").parse::<u32>() {
            Ok(count) if (1..=MOST_TO_MAKE).contains(&count) => ChainOrder::Make(count),
            _ => return Err(format!("Say how many blocks, from 1 to {MOST_TO_MAKE}, or stop.")),
        },
    };
    order(desk, asked)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::words::run_line;
    use crate::core::BoardView;

    #[derive(Default)]
    struct Heard(Vec<ChainOrder>);

    impl Desk for Heard {
        fn command(&mut self, command: Command) -> Result<String, String> {
            if let Command::Chain(order) = command {
                self.0.push(order);
            }
            Ok(String::new())
        }
        fn boards(&mut self) -> Result<Vec<BoardView>, String> {
            Ok(Vec::new())
        }
        fn password(&mut self) -> Result<String, String> {
            Err("none".into())
        }
    }

    #[test]
    fn the_chain_words_end_in_their_orders() {
        let mut desk = Heard::default();
        for line in ["mempool", "coins", "generate 5", "generate", "generate stop", "generate 1,000"] {
            run_line(&mut desk, line).unwrap_or_else(|said| panic!("{line}: {said}"));
        }
        use ChainOrder::*;
        assert_eq!(desk.0, [Waiting, Coins, Make(5), MakeShow, MakeStop, Make(1_000)]);
        for wrong in ["generate 0", "generate 1001", "generate many"] {
            assert!(run_line(&mut desk, wrong).is_err(), "{wrong}");
        }
    }
}
