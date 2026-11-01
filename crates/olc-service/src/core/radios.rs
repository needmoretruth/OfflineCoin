//! Links that need a radio of this computer, the same for `olc` and OLC Core. What a system cannot
//! do is said in plain words and costs nothing else.
use olc_node::Runner;
use olc_platform::platform;
use olc_store::NodeSettings;

pub const NO_BLUETOOTH: &str = "This computer cannot use Bluetooth.";
pub const NO_ESP_NOW: &str = "This computer cannot use ESP-NOW.";

/// Whether this build on this system can use Bluetooth.
pub fn bluetooth_here() -> bool {
    cfg!(feature = "ble") && platform().radio_links()
}

#[cfg(all(feature = "ble", target_os = "linux"))]
pub fn add_bluetooth(runner: &mut Runner) -> Result<(), String> {
    runner.add_link(Box::new(olc_link_ble::BleLink::start(runner.id())?));
    Ok(())
}

#[cfg(not(all(feature = "ble", target_os = "linux")))]
pub fn add_bluetooth(_runner: &mut Runner) -> Result<(), String> {
    Err(NO_BLUETOOTH.into())
}

#[cfg(target_os = "linux")]
pub fn add_esp_now(runner: &mut Runner, card: &str) -> Result<(), String> {
    runner.add_link(Box::new(olc_link_espnow::EspNowCard::open(card, runner.mark())?));
    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub fn add_esp_now(_runner: &mut Runner, _card: &str) -> Result<(), String> {
    Err(NO_ESP_NOW.into())
}

/// Which radios a node has running, so that a change of coin does not start them twice.
#[derive(Default)]
pub struct Running {
    bluetooth: bool,
    card: String,
}

impl Running {
    /// Starts what the settings ask for and is not running yet. Says what could not start.
    pub fn bring_up(&mut self, runner: &mut Runner, settings: &NodeSettings) -> Vec<String> {
        let mut problems = Vec::new();
        if settings.bluetooth && !self.bluetooth {
            self.bluetooth = true;
            problems.extend(add_bluetooth(runner).err());
        }
        if !settings.esp_now_card.is_empty() && self.card != settings.esp_now_card {
            self.card = settings.esp_now_card.clone();
            problems.extend(add_esp_now(runner, &self.card).err());
        }
        problems
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_build_without_bluetooth_says_so_in_plain_words() {
        if !cfg!(feature = "ble") {
            assert!(!bluetooth_here());
        }
    }
}
