//! The command line's words for seed words, signed messages, shared wallets and speeding up a
//! payment. Each one sends the same order a button does.
use super::commands::{Command, LockOrder, Personal};
use super::wallet_orders::WalletOrder;
use super::words::Desk;
use olc_board::Args;

fn own(desk: &mut dyn Desk, order: WalletOrder) -> Result<String, String> {
    desk.command(Command::Own(Personal::Wallet(order)))
}

/// Everything from word `from` on.
fn rest(args: &Args, from: usize) -> String {
    (from..).map_while(|index| args.word(index)).collect::<Vec<_>>().join(" ")
}

fn number(word: Option<&str>, saying: &str) -> Result<usize, String> {
    word.and_then(|word| word.parse().ok()).ok_or_else(|| saying.to_string())
}

pub(super) fn words(desk: &mut dyn Desk, args: &Args) -> Result<String, String> {
    match args.word(1) {
        None => own(desk, WalletOrder::ShowWords),
        Some("written") => own(desk, WalletOrder::WordsWritten),
        Some("new") => {
            let korean = args.word(2) == Some("korean");
            let made = own(desk, WalletOrder::NewWords { korean })?;
            let words = own(desk, WalletOrder::ShowWords)?;
            Ok(format!("{made}\n{words}"))
        }
        Some("restore") if args.word(2).is_some() => own(desk, WalletOrder::RestoreWords(rest(args, 2))),
        _ => Err("Use: olc words | words written | words new [korean] | words restore WORD...".into()),
    }
}

pub(super) fn sign_message(desk: &mut dyn Desk, args: &Args) -> Result<String, String> {
    let message = rest(args, 1);
    if message.is_empty() {
        return Err("Write the message: olc sign-message [--address ADDRESS] TEXT".into());
    }
    let address = args.value("address").unwrap_or_default().to_string();
    own(desk, WalletOrder::SignMessage { address, message })
}

pub(super) fn verify_message(desk: &mut dyn Desk, args: &Args) -> Result<String, String> {
    let (Some(address), Some(signature)) = (args.word(1), args.word(2)) else {
        return Err("Use: olc verify-message ADDRESS SIGNATURE TEXT".into());
    };
    let (address, signature, message) = (address.to_string(), signature.to_string(), rest(args, 3));
    own(desk, WalletOrder::VerifyMessage { address, signature, message })
}

pub(super) fn shared(desk: &mut dyn Desk, args: &Args) -> Result<String, String> {
    match args.word(1) {
        None => own(desk, WalletOrder::ListShared),
        Some("new") => {
            let need = number(args.word(2), "Say how many must sign: olc shared new 2 KEY KEY KEY")?;
            let keys = (3..).map_while(|index| args.word(index)).map(str::to_string).collect();
            own(desk, WalletOrder::MakeShared { need, keys })
        }
        Some("add") if args.word(2).is_some() => own(desk, WalletOrder::AddShared(rest(args, 2))),
        Some("forget") => own(desk, WalletOrder::ForgetShared(number(args.word(2), "Name the shared wallet by its number.")?)),
        Some("pay") => {
            let wallet = number(args.word(2), "Use: olc shared pay N ADDRESS AMOUNT")?;
            let (Some(to), Some(amount)) = (args.word(3), args.word(4)) else {
                return Err("Use: olc shared pay N ADDRESS AMOUNT".into());
            };
            let (to, amount) = (to.to_string(), amount.to_string());
            desk.command(Command::Lock(LockOrder::PayShared { wallet, to, amount }))
        }
        _ => Err("Use: olc shared | shared new N KEY... | add TEXT | forget N | pay N ADDRESS AMOUNT".into()),
    }
}

pub(super) fn speed_up(desk: &mut dyn Desk, args: &Args) -> Result<String, String> {
    let id = args.word(1).ok_or("Name the waiting payment by its id.")?.to_string();
    desk.command(Command::Lock(LockOrder::SpeedUp { id }))
}
