//! ESP-NOW is plain vendor action frames on a Wi-Fi channel, so a Linux card in monitor mode can
//! hear boards and speak to them without any Espressif code.
pub mod air;
#[cfg(feature = "card")]
mod card;
#[cfg(feature = "card")]
pub use card::EspNowCard;
