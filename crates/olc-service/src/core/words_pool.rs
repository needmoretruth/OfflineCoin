//! The words for pools. Each ends in the same [`PoolOrder`] the pool page sends.
use super::commands::Command;
use super::pool_plan::{pool_address, Join, Method, PoolOrder, PoolSettings, Who};
use super::words::{find_board, Desk};
use olc_board::Args;
use olc_pool::{worker_address, MOST_FEE_BPS};

fn order(desk: &mut dyn Desk, order: PoolOrder) -> Result<String, String> {
    desk.command(Command::Pool(order))
}

/// `own`, a node's number, or a board's name or id.
fn who_of(desk: &mut dyn Desk, word: Option<&str>) -> Result<Who, String> {
    match word {
        None => Err("Say which miner: own, a node's number or a board's name.".into()),
        Some("own") => Ok(Who::Own),
        Some(word) => match word.parse() {
            Ok(id) => Ok(Who::Node(id)),
            Err(_) => find_board(desk, Some(word)).map(Who::Board),
        },
    }
}

/// A percent as hundredths of a percent: `1.5` is 150.
fn fee_of(text: &str) -> Result<u32, String> {
    let percent: f64 = text.trim().trim_end_matches('%').parse().map_err(|_| "Give the fee as a percent, such as 1.5.")?;
    let bps = (percent * 100.0).round();
    if !(0.0..=f64::from(MOST_FEE_BPS)).contains(&bps) {
        return Err("A pool's fee is from 0 to 5 percent.".into());
    }
    Ok(bps as u32)
}

pub(super) fn pool_start(desk: &mut dyn Desk, args: &Args) -> Result<String, String> {
    let mut settings = PoolSettings::default();
    if let Some(port) = args.value("port") {
        settings.port = port.parse().ok().filter(|port| *port > 0).ok_or("Give a port from 1 to 65535.")?;
    }
    settings.open = args.has("open");
    if let Some(shares) = args.value("pplns") {
        settings.method = Method::LastShares;
        settings.last_shares = shares.parse().ok().filter(|shares| *shares > 0).ok_or("Give how many shares PPLNS counts.")?;
    }
    if let Some(fee) = args.value("fee") {
        settings.fee_bps = fee_of(fee)?;
    }
    if let Some(difficulty) = args.value("difficulty") {
        settings.fixed_difficulty = difficulty.parse().map_err(|_| "Give the share difficulty as a number.")?;
    }
    if let Some(workers) = args.value("workers") {
        let mut chosen = Vec::new();
        for word in workers.split(',').filter(|word| !word.is_empty()) {
            chosen.push(who_of(desk, Some(word))?);
        }
        settings.workers = chosen;
    }
    order(desk, PoolOrder::Start(settings))
}

pub(super) fn pool(desk: &mut dyn Desk, _args: &Args) -> Result<String, String> {
    order(desk, PoolOrder::Show)
}

pub(super) fn pool_stop(desk: &mut dyn Desk, _args: &Args) -> Result<String, String> {
    order(desk, PoolOrder::Stop)
}

pub(super) fn pool_join(desk: &mut dyn Desk, args: &Args) -> Result<String, String> {
    let who = who_of(desk, args.word(1))?;
    let address = args.word(2).and_then(pool_address).ok_or("Give the pool's address, HOST:PORT.")?;
    let user = args.word(3).ok_or("Give the name to sign in with, ADDRESS.WORKER.")?;
    worker_address(user).map_err(str::to_string)?;
    order(desk, PoolOrder::Join(Join { who, address, user: user.to_string() }))
}

pub(super) fn pool_leave(desk: &mut dyn Desk, args: &Args) -> Result<String, String> {
    let who = who_of(desk, args.word(1))?;
    order(desk, PoolOrder::Leave(who))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::words::run_line;
    use crate::core::BoardView;

    #[derive(Default)]
    struct Heard(Vec<PoolOrder>);

    impl Desk for Heard {
        fn command(&mut self, command: Command) -> Result<String, String> {
            if let Command::Pool(order) = command {
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
    fn a_pool_starts_joins_and_leaves_from_a_line() {
        let mut desk = Heard::default();
        run_line(&mut desk, "pool-start").unwrap();
        let PoolOrder::Start(settings) = &desk.0[0] else { panic!("{:?}", desk.0) };
        assert_eq!((settings.port, settings.open, settings.workers.clone()), (3333, false, vec![Who::Own]));
        run_line(&mut desk, "pool-start --open --port 4000 --pplns 500 --fee 1.5 --workers own,2").unwrap();
        let PoolOrder::Start(settings) = &desk.0[1] else { panic!("{:?}", desk.0) };
        assert_eq!((settings.port, settings.open, settings.method, settings.last_shares), (4000, true, Method::LastShares, 500));
        assert_eq!((settings.fee_bps, settings.workers.clone()), (150, vec![Who::Own, Who::Node(2)]));
        assert!(run_line(&mut desk, "pool-start --fee 7").is_err(), "more than 5 %");
        let user = format!("{}.rig", olc_wallet::format_address(&[7; 20]));
        run_line(&mut desk, &format!("pool-join own 192.168.0.5 {user}")).unwrap();
        let joined = PoolOrder::Join(Join { who: Who::Own, address: "192.168.0.5:3333".into(), user });
        assert_eq!(desk.0.last(), Some(&joined));
        assert!(run_line(&mut desk, "pool-join own 192.168.0.5 nobody").is_err());
        run_line(&mut desk, "pool-leave 3").unwrap();
        assert_eq!(desk.0.last(), Some(&PoolOrder::Leave(Who::Node(3))));
    }
}
