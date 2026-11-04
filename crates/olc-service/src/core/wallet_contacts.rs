//! The address book: other people's addresses by name, as Bitcoin Core keeps its sending
//! addresses. Contacts live in the wallet file beside the names of the wallet's own addresses, so
//! they travel with it and its backups, and a locked wallet still changes them.
use super::ops::Book;
use olc_wallet::{format_address, parse_address, Contact, Wallet};

/// A name is a few words, as a label is.
pub const MOST_NAME: usize = 60;

/// Something to do with the address book.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ContactOrder {
    Add { name: String, address: String },
    Rename { name: String, new_name: String },
    Remove(String),
    List,
}

fn same_name(one: &str, other: &str) -> bool {
    one.to_lowercase() == other.to_lowercase()
}

fn clean_name(name: &str) -> Result<String, String> {
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    match name.is_empty() || name.chars().count() > MOST_NAME {
        true => Err("notice.contact_name_bad".to_string()),
        false => Ok(name),
    }
}

/// The address as the wallet writes it, so one address saved twice is seen as one.
fn clean_address(text: &str) -> Result<String, String> {
    parse_address(text.trim()).map(|address| format_address(&address)).ok_or_else(|| "notice.bad_address".to_string())
}

fn sort(wallet: &mut Wallet) {
    wallet.contacts.sort_by_key(|contact| contact.name.to_lowercase());
}

fn add(wallet: &mut Wallet, name: &str, address: &str) -> Result<(&'static str, String), String> {
    let (name, address) = (clean_name(name)?, clean_address(address)?);
    // Not a failure: the address is already where the person wanted it, under the name it has.
    if let Some(known) = wallet.contacts.iter().find(|contact| contact.address == address) {
        return Ok(("notice.contact_known", known.name.clone()));
    }
    if wallet.contacts.iter().any(|contact| same_name(&contact.name, &name)) {
        return Err("notice.contact_name_taken".to_string());
    }
    wallet.contacts.push(Contact { name: name.clone(), address });
    sort(wallet);
    Ok(("notice.contact_saved", name))
}

fn rename(wallet: &mut Wallet, name: &str, new_name: &str) -> Result<(&'static str, String), String> {
    let new_name = clean_name(new_name)?;
    let taken = wallet.contacts.iter().any(|contact| same_name(&contact.name, &new_name) && !same_name(&contact.name, name));
    if taken {
        return Err("notice.contact_name_taken".to_string());
    }
    let contact = wallet.contacts.iter_mut().find(|contact| same_name(&contact.name, name.trim()));
    contact.ok_or("notice.no_contact")?.name = new_name.clone();
    sort(wallet);
    Ok(("notice.contact_saved", new_name))
}

fn remove(wallet: &mut Wallet, name: &str) -> Result<(&'static str, String), String> {
    let at = wallet.contacts.iter().position(|contact| same_name(&contact.name, name.trim()));
    let gone = wallet.contacts.remove(at.ok_or("notice.no_contact")?);
    Ok(("notice.contact_removed", gone.name))
}

/// One line for each contact: its name and its address.
fn list(wallet: &Wallet) -> String {
    let rows: Vec<String> = wallet.contacts.iter().map(|contact| format!("{}  {}", contact.name, contact.address)).collect();
    match rows.is_empty() {
        true => "No contacts yet. Add one: olc contact add NAME ADDRESS".to_string(),
        false => rows.join("\n"),
    }
}

/// Adds contacts from a backup that this wallet lacks. One whose name or address is already here
/// stays as this wallet has it.
pub(super) fn take_in(wallet: &mut Wallet, contacts: &[Contact]) {
    for contact in contacts {
        let known = wallet.contacts.iter().any(|own| own.address == contact.address || same_name(&own.name, &contact.name));
        if !known {
            wallet.contacts.push(contact.clone());
        }
    }
    sort(wallet);
}

pub(super) fn carry_contact<B: Book + ?Sized>(book: &B, order: ContactOrder) -> Result<(&'static str, String), String> {
    match order {
        ContactOrder::Add { name, address } => book.wallet_change(|wallet| add(wallet, &name, &address)),
        ContactOrder::Rename { name, new_name } => book.wallet_change(|wallet| rename(wallet, &name, &new_name)),
        ContactOrder::Remove(name) => book.wallet_change(|wallet| remove(wallet, &name)),
        ContactOrder::List => book.wallet_view(list).map(|rows| ("notice.contacts", rows)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn address(seed: u8) -> String {
        format_address(&Wallet::from_secret([seed; 32]).unwrap().address())
    }

    #[test]
    fn a_contact_is_kept_once_by_name_and_once_by_address() {
        let mut wallet = Wallet::from_secret([1; 32]).unwrap();
        assert_eq!(add(&mut wallet, "  Mina  Kim ", &address(2)).unwrap().1, "Mina Kim");
        assert_eq!(add(&mut wallet, "mina kim", &address(3)).err().as_deref(), Some("notice.contact_name_taken"));
        assert_eq!(add(&mut wallet, "Jun", &address(2)), Ok(("notice.contact_known", "Mina Kim".to_string())));
        assert_eq!(wallet.contacts.len(), 1);
        assert_eq!(add(&mut wallet, "Jun", "olc1nothing").err().as_deref(), Some("notice.bad_address"));
        assert_eq!(add(&mut wallet, " ", &address(3)).err().as_deref(), Some("notice.contact_name_bad"));
        add(&mut wallet, "Ari", &address(3)).unwrap();
        assert_eq!(wallet.contacts.iter().map(|contact| contact.name.as_str()).collect::<Vec<_>>(), ["Ari", "Mina Kim"]);
        rename(&mut wallet, "mina kim", "Mina").unwrap();
        assert_eq!(rename(&mut wallet, "Mina", "ari").err().as_deref(), Some("notice.contact_name_taken"));
        assert_eq!(remove(&mut wallet, "Nobody").err().as_deref(), Some("notice.no_contact"));
        assert_eq!(remove(&mut wallet, "ARI").unwrap().1, "Ari");
        assert_eq!(list(&wallet), format!("Mina  {}", address(2)));
    }
}
