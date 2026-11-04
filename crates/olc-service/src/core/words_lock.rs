//! The command line's words for the wallet's password and its contacts, as bitcoin-cli's
//! encryptwallet, walletpassphrase, walletlock, walletpassphrasechange and the address book.
//! Passwords are asked for out of sight and never ride on the command line.
use super::commands::{Command, Personal};
use super::wallet_contacts::ContactOrder;
use super::wallet_lock::{Password, PasswordOrder, OPEN_FOR};
use super::wallet_orders::WalletOrder;
use super::words::Desk;
use olc_board::Args;
use olc_control::line::split;

pub const WALLET_USAGE: &str = "Use: olc wallet | wallet encrypt | wallet unlock [SECONDS] | wallet lock | wallet password";
const CONTACT_USAGE: &str = "Use: olc contact add NAME ADDRESS | contact rename NAME NEW | contact remove NAME";
const DIFFERENT: &str = "The two passwords differ. Nothing changed.";

fn own(desk: &mut dyn Desk, order: WalletOrder) -> Result<String, String> {
    desk.command(Command::Own(Personal::Wallet(order)))
}

/// A new password, typed twice so a slip of the finger does not lock a person out.
pub fn new_password(first: Password, again: Password) -> Result<Password, String> {
    match first == again {
        true => Ok(first),
        false => Err(DIFFERENT.to_string()),
    }
}

/// How many passwords `wallet` reads after its second word: a new one twice, the one that opens
/// it, or the old one and a new one twice.
fn wallet_passwords(second: Option<&str>) -> usize {
    match second {
        Some("encrypt") => 2,
        Some("unlock") => 1,
        Some("password") => 3,
        _ => 0,
    }
}

/// `wallet`: how it stands, or one of the four password words.
pub(super) fn wallet(desk: &mut dyn Desk, args: &Args) -> Result<String, String> {
    // Every password the line asks for is read before anything can fail, so a terminal reading
    // them out of sight never takes a password for the next word.
    let mut typed = Vec::new();
    for _ in 0..wallet_passwords(args.word(1)) {
        typed.push(Password(desk.password()?));
    }
    let mut typed = typed.into_iter();
    let mut next = || typed.next().unwrap_or_default();
    let order = match args.word(1) {
        None => PasswordOrder::State,
        Some("encrypt") => PasswordOrder::Encrypt(new_password(next(), next())?),
        Some("unlock") => {
            let password = next();
            let seconds = match args.word(2) {
                None => OPEN_FOR,
                Some(word) => word.parse().map_err(|_| "Say how many seconds: olc wallet unlock 300")?,
            };
            PasswordOrder::Unlock { password, seconds }
        }
        Some("lock") => PasswordOrder::Lock,
        Some("password") => {
            let old = next();
            PasswordOrder::Change { old, new: new_password(next(), next())? }
        }
        _ => return Err(WALLET_USAGE.into()),
    };
    own(desk, WalletOrder::Password(order))
}

/// How many passwords a typed line asks for, so a terminal reads exactly those without showing
/// them: the wallet's words, and a backup or restore with `--password`, as their words read them.
pub fn passwords_asked(line: &str) -> usize {
    let args = Args::parse(split(line).into_iter());
    match args.command() {
        Some("wallet") => wallet_passwords(args.word(1)),
        Some("backup" | "restore") => usize::from(args.word(1).is_some() && args.has("password")),
        _ => 0,
    }
}

pub(super) fn contacts(desk: &mut dyn Desk, _args: &Args) -> Result<String, String> {
    own(desk, WalletOrder::Contact(ContactOrder::List))
}

/// Everything from word `from` on, for a name with spaces in it.
fn rest(args: &Args, from: usize) -> String {
    (from..).map_while(|index| args.word(index)).collect::<Vec<_>>().join(" ")
}

pub(super) fn contact(desk: &mut dyn Desk, args: &Args) -> Result<String, String> {
    let order = match (args.word(1), args.word(2), args.word(3)) {
        (Some("add"), Some(name), Some(address)) => {
            // The address is the last word, so a name may have spaces in it.
            let words: Vec<&str> = (2..).map_while(|index| args.word(index)).collect();
            let name = match words.len() > 2 {
                true => words[..words.len() - 1].join(" "),
                false => name.to_string(),
            };
            let address = words.last().copied().unwrap_or(address).to_string();
            ContactOrder::Add { name, address }
        }
        (Some("rename"), Some(name), Some(new_name)) => {
            ContactOrder::Rename { name: name.to_string(), new_name: format!("{new_name} {}", rest(args, 4)).trim().to_string() }
        }
        (Some("remove"), Some(_), _) => ContactOrder::Remove(rest(args, 2)),
        _ => return Err(CONTACT_USAGE.into()),
    };
    own(desk, WalletOrder::Contact(order))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::snapshot::BoardView;
    use crate::core::words::run_line;

    /// Hands out typed passwords in turn and keeps the wallet orders it is given.
    #[derive(Default)]
    struct Typed {
        passwords: Vec<&'static str>,
        read: usize,
        orders: Vec<WalletOrder>,
    }

    impl Desk for Typed {
        fn command(&mut self, command: Command) -> Result<String, String> {
            if let Command::Own(Personal::Wallet(order)) = command {
                self.orders.push(order);
            }
            Ok(String::new())
        }

        fn boards(&mut self) -> Result<Vec<BoardView>, String> {
            Ok(Vec::new())
        }

        fn password(&mut self) -> Result<String, String> {
            self.read += 1;
            Ok(self.passwords.get(self.read - 1).copied().unwrap_or_default().to_string())
        }
    }

    fn typed(line: &str, passwords: &[&'static str]) -> (Typed, Result<String, String>) {
        let mut desk = Typed { passwords: passwords.to_vec(), ..Typed::default() };
        let said = run_line(&mut desk, line);
        (desk, said)
    }

    /// A terminal reads as many lines out of sight as the word takes, whatever the word then
    /// says, so a password never runs as the next word.
    #[test]
    fn each_word_reads_the_passwords_a_terminal_hides_and_no_more() {
        for line in ["wallet encrypt", "wallet unlock 60", "wallet unlock soon", "wallet password", "wallet", "wallet lock", "backup a.backup --password", "restore --password"] {
            let (desk, _) = typed(line, &["one", "one", "one"]);
            assert_eq!(desk.read, passwords_asked(line), "{line}");
        }
        assert_eq!(passwords_asked("wallet encrypt"), 2);
        assert_eq!(passwords_asked("pay olc1 5"), 0);
    }

    #[test]
    fn the_wallet_words_carry_their_passwords_and_refuse_two_that_differ() {
        let (desk, said) = typed("wallet unlock 60", &["open"]);
        assert!(said.is_ok());
        let unlock = PasswordOrder::Unlock { password: Password("open".into()), seconds: 60 };
        assert_eq!(desk.orders, [WalletOrder::Password(unlock)]);
        let (desk, said) = typed("wallet encrypt", &["new", "wen"]);
        assert_eq!(said.err().as_deref(), Some(DIFFERENT));
        assert!(desk.orders.is_empty(), "nothing goes with two passwords that differ");
        let (desk, _) = typed("wallet password", &["old", "new", "new"]);
        let change = PasswordOrder::Change { old: Password("old".into()), new: Password("new".into()) };
        assert_eq!(desk.orders, [WalletOrder::Password(change)]);
        assert_eq!(format!("{:?}", desk.orders), "[Password(Change { old: Password(..), new: Password(..) })]");
    }

    #[test]
    fn contact_names_may_have_spaces() {
        let (desk, _) = typed("contact add Mina Kim olc1abc", &[]);
        assert_eq!(desk.orders, [WalletOrder::Contact(ContactOrder::Add { name: "Mina Kim".into(), address: "olc1abc".into() })]);
        let (desk, _) = typed("contact rename Mina Mina Kim", &[]);
        assert_eq!(desk.orders, [WalletOrder::Contact(ContactOrder::Rename { name: "Mina".into(), new_name: "Mina Kim".into() })]);
        let (desk, _) = typed("contact remove Mina Kim", &[]);
        assert_eq!(desk.orders, [WalletOrder::Contact(ContactOrder::Remove("Mina Kim".into()))]);
        assert!(typed("contact add Mina", &[]).1.is_err());
    }
}
