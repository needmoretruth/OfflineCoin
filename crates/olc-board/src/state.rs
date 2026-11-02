//! What a board reports about its own settings, read from its status line.
use std::collections::BTreeMap;
use std::str::FromStr;

/// `None` is what the board's firmware does not say, or has not said yet.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BoardState {
    /// The panel's stored brightness.
    pub bright: Option<u8>,
    /// Talking to nearby boards by radio.
    pub nearby: Option<bool>,
    pub bluetooth: Option<bool>,
    /// Running its own Wi-Fi network.
    pub own_network: Option<bool>,
    /// Joined to a router.
    pub on_wifi: Option<bool>,
    pub merging: Option<bool>,
    /// Holding headers only.
    pub headers_only: Option<bool>,
    pub clock: Option<u32>,
    pub power: Option<i32>,
    /// Blocks it keeps, with 0 for all of them.
    pub keep: Option<u64>,
    /// Which link it tries first, in order.
    pub links: Option<String>,
    /// Where its mined coins go.
    pub miner: Option<String>,
    /// The short name of the rules it runs by. A build from before this word says nothing.
    pub rules: Option<String>,
}

impl BoardState {
    pub(crate) fn parse(fields: &BTreeMap<&str, &str>) -> Self {
        fn read<T: FromStr>(fields: &BTreeMap<&str, &str>, key: &str) -> Option<T> {
            fields.get(key).and_then(|word| word.parse().ok())
        }
        Self {
            bright: read(fields, "bright"),
            nearby: read(fields, "nearby"),
            bluetooth: read(fields, "bluetooth"),
            own_network: fields.get("network").map(|word| *word == "on"),
            on_wifi: fields.get("router").map(|word| *word != "None"),
            merging: read(fields, "merge"),
            headers_only: read(fields, "headers_only"),
            clock: read(fields, "clock"),
            power: read(fields, "power"),
            keep: read(fields, "keep"),
            links: read(fields, "links"),
            miner: read(fields, "miner"),
            rules: read(fields, "rules"),
        }
    }

    /// What a line the board took changes, until it next reports.
    pub fn after(&mut self, line: &str) {
        let mut words = line.split_whitespace();
        let (Some(word), Some(value)) = (words.next(), words.next()) else {
            return;
        };
        let on = Some(value == "on");
        match word {
            "bright" => self.bright = value.parse().ok().or(self.bright),
            "espnow" => self.nearby = on,
            "bluetooth" => self.bluetooth = on,
            "network" => self.own_network = on,
            "wifi" if value == "off" => self.on_wifi = Some(false),
            "merge" => self.merging = on,
            "light" => self.headers_only = on,
            "clock" => self.clock = value.parse().ok().or(self.clock),
            "power" => self.power = value.parse().ok().or(self.power),
            "keep" if value == "all" => self.keep = Some(0),
            "keep" => self.keep = value.parse().ok().or(self.keep),
            "links" => self.links = Some(value.to_string()),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields(line: &str) -> BTreeMap<&str, &str> {
        line.split_whitespace().filter_map(|word| word.split_once('=')).collect()
    }

    #[test]
    fn a_board_says_what_is_on() {
        let line = "network=off router=Some(192.168.0.7) nearby=true bluetooth=false merge=true \
                    headers_only=false clock=160 power=8 keep=1000 links=wired,wifi bright=96";
        let state = BoardState::parse(&fields(line));
        assert_eq!(state.nearby, Some(true));
        assert_eq!(state.bluetooth, Some(false));
        assert_eq!(state.own_network, Some(false));
        assert_eq!(state.on_wifi, Some(true));
        assert_eq!(state.merging, Some(true));
        assert_eq!(state.headers_only, Some(false));
        assert_eq!((state.clock, state.power, state.keep), (Some(160), Some(8), Some(1000)));
        assert_eq!(state.links.as_deref(), Some("wired,wifi"));
    }

    #[test]
    fn what_older_firmware_leaves_out_stays_unknown() {
        let state = BoardState::parse(&fields("network=on router=None"));
        assert_eq!((state.own_network, state.on_wifi), (Some(true), Some(false)));
        assert_eq!((state.nearby, state.bluetooth, state.headers_only), (None, None, None));
    }

    #[test]
    fn a_taken_line_changes_only_its_own_setting() {
        let mut state = BoardState { nearby: Some(true), bright: Some(10), ..BoardState::default() };
        state.after("espnow off");
        state.after("bright 96");
        state.after("keep all");
        state.after("wifi off");
        state.after("wifi Home secret");
        state.after("mine on");
        assert_eq!(state.nearby, Some(false));
        assert_eq!(state.bright, Some(96));
        assert_eq!(state.keep, Some(0));
        assert_eq!(state.on_wifi, Some(false));
        assert_eq!(state.bluetooth, None);
    }
}
