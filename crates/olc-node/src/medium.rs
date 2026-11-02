//! The kinds of link there are, and the one ordered list that says which is best.
use alloc::string::String;
use alloc::vec::Vec;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Medium {
    /// A cable: USB or jumper wires.
    Wire,
    EspNow,
    Bluetooth,
    /// TCP, which on a board means Wi-Fi.
    Wifi,
}

impl Medium {
    pub const ALL: [Medium; 4] = [Medium::Wire, Medium::EspNow, Medium::Bluetooth, Medium::Wifi];

    pub fn name(self) -> &'static str {
        match self {
            Medium::Wire => "wired",
            Medium::EspNow => "espnow",
            Medium::Bluetooth => "bluetooth",
            Medium::Wifi => "wifi",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|medium| medium.name() == name)
    }
}

/// Best first. A setting, so an owner can swap any two.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rank(pub [Medium; 4]);

impl Default for Rank {
    /// From measurements between real boards. Wires lost nothing and need no radio, and Wi-Fi
    /// carried eight times what Bluetooth did, while staying off until a person turns it on.
    fn default() -> Self {
        Self([Medium::Wire, Medium::EspNow, Medium::Wifi, Medium::Bluetooth])
    }
}

impl Rank {
    pub fn place_of(&self, medium: Medium) -> usize {
        self.0.iter().position(|one| *one == medium).unwrap_or(usize::MAX)
    }

    /// Reads `wired,espnow,wifi,bluetooth`. Each must appear once.
    pub fn parse(text: &str) -> Option<Self> {
        let list: Option<Vec<Medium>> = text.split(',').map(|name| Medium::from_name(name.trim())).collect();
        let list: [Medium; 4] = list?.try_into().ok()?;
        let whole = Medium::ALL.iter().all(|medium| list.contains(medium));
        whole.then_some(Self(list))
    }

    pub fn text(&self) -> String {
        let names: Vec<&str> = self.0.iter().map(|medium| medium.name()).collect();
        names.join(",")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_text_form_reads_back() {
        let rank = Rank::parse("espnow, wired,wifi,bluetooth").unwrap();
        assert_eq!(rank.place_of(Medium::EspNow), 0);
        assert_eq!(Rank::parse(&rank.text()), Some(rank));
    }

    #[test]
    fn every_medium_must_appear_once() {
        assert_eq!(Rank::parse("wired,espnow,bluetooth"), None);
        assert_eq!(Rank::parse("wired,wired,bluetooth,wifi"), None);
        assert_eq!(Rank::parse("wired,espnow,bluetooth,carrier-pigeon"), None);
    }
}
