//! Links that need a radio of this computer, the same for `olc` and OLC Core. What a system cannot
//! do is said in plain words and costs nothing else.
use olc_node::Runner;
use olc_platform::platform;
pub use olc_platform::{NoBluetooth, NO_BLUETOOTH, NO_ESP_NOW};
use olc_store::NodeSettings;


/// Whether this build on this system can use Bluetooth.
pub fn bluetooth_here() -> bool {
    platform().has_bluetooth()
}

/// Why it cannot.
pub fn no_bluetooth() -> Option<NoBluetooth> {
    platform().no_bluetooth()
}

fn add_bluetooth(runner: &mut Runner) -> Result<(), String> {
    runner.add_link(platform().bluetooth_link(runner.id())?);
    Ok(())
}

fn add_esp_now(runner: &mut Runner, card: &str) -> Result<(), String> {
    runner.add_link(platform().esp_now_link(card, runner.mark())?);
    Ok(())
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
