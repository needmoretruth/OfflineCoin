//! Opening a channel from one address: `KEY@HOST:PORT` dials the node if it is not heard yet and
//! opens once it says hello, as Lightning's `connect` then `fundchannel` do in one step.
use super::{channels_from, hex, refusal_key, short_id, ChannelDesk, NodeGround, TCP_LINK};
use crate::core::coins::parse_coins;
use crate::core::host_parts::unix_ms;
use olc_channel::bytes::from_hex;
use olc_channel::node::DELAY;
use olc_channel::opening::{FEWEST_DELAY, MOST_DELAY};
use olc_channel::{Bytes, NodeId, Refusal};
use olc_node::Runner;

/// The port a node listens on unless told otherwise, for an address written without one.
const USUAL_PORT: u16 = crate::settings::NODE_PORT;
/// How long a node dialled by address has to say hello before the open is given up.
const REACH_WAIT_MS: u64 = 60_000;

/// A channel asked for with a node not reached yet: it opens once that node says hello.
#[derive(Clone, Debug)]
pub(super) struct Waiting {
    key: NodeId,
    pub(super) address: String,
    amount: u64,
    push: u64,
    delay: u16,
    until_ms: u64,
}

/// A channel address read: the node's whole key, and where to dial it if given. `key@host` takes
/// the usual port, as CLN's `connect` does.
pub fn read_channel_address(text: &str) -> Result<(NodeId, Option<String>), &'static str> {
    let text = text.trim();
    let (key, at) = match text.split_once('@') {
        Some((key, at)) => (key.trim(), Some(at.trim())),
        None => (text, None),
    };
    let bytes = from_hex(key).filter(|bytes| bytes.len() == 33).ok_or("ch.bad_key")?;
    let mut node = [0u8; 33];
    node.copy_from_slice(&bytes);
    let address = match at {
        None => None,
        Some("") => return Err("ch.bad_address"),
        Some(at) if at.parse::<std::net::SocketAddr>().is_ok() => Some(at.to_string()),
        Some(at) if at.parse::<std::net::Ipv6Addr>().is_ok() => Some(format!("[{at}]:{USUAL_PORT}")),
        // A host with no port.
        Some(at) if !at.contains(':') => Some(format!("{at}:{USUAL_PORT}")),
        Some(at) => match at.rsplit_once(':').map(|(_, port)| port.parse::<u16>()) {
            Some(Ok(_)) => Some(at.to_string()),
            _ => return Err("ch.bad_address"),
        },
    };
    Ok((Bytes(node), address))
}

/// The usual wait, or the one typed when a node takes it.
fn read_delay(text: &str) -> Option<u16> {
    match text.trim() {
        "" => Some(DELAY),
        typed => typed.parse::<u16>().ok().filter(|blocks| (FEWEST_DELAY..=MOST_DELAY).contains(blocks)),
    }
}

/// Why an open was refused, in the words the screen and the terminal show.
fn refused(runner: &Runner, refusal: Refusal) -> (&'static str, String, bool) {
    match refusal {
        Refusal::NoV2Yet => match channels_from(runner) {
            Some(height) => ("ch.no_v2", height.to_string(), false),
            None => ("ch.no_v2_never", String::new(), false),
        },
        other => (refusal_key(other), String::new(), false),
    }
}

impl ChannelDesk {
    /// Opens a channel to `peer`: a name or key start of a node heard lately, a whole key, or a
    /// whole key with the address to dial it at.
    pub(super) fn open_order(
        &mut self,
        runner: &mut Runner,
        peer: &str,
        amount: &str,
        push: &str,
        delay: &str,
    ) -> (&'static str, String, bool) {
        let Some(node) = self.node.as_mut() else {
            return ("ch.no_running", String::new(), false);
        };
        let Some(units) = parse_coins(amount.trim()).filter(|units| *units > 0) else {
            return ("notice.bad_amount", String::new(), false);
        };
        let push = match push.trim() {
            "" => Some(0),
            typed => parse_coins(typed).filter(|pushed| *pushed < units),
        };
        let Some(push) = push else {
            return ("ch.bad_push", String::new(), false);
        };
        let Some(delay) = read_delay(delay) else {
            return ("ch.no_delay", String::new(), false);
        };
        let peer = peer.trim();
        let (to, address) = match node.find_peer(peer) {
            Some(to) if !peer.contains('@') => (to, None),
            _ => match read_channel_address(peer) {
                Ok(read) => read,
                Err(key) if peer.contains('@') => return (key, peer.to_string(), false),
                Err(_) => return ("ch.not_connected", peer.to_string(), false),
            },
        };
        if to == node.node_key() {
            return refused(runner, Refusal::ThisNode);
        }
        let heard = node.peers().iter().any(|(key, _)| *key == to);
        match (heard, address) {
            (true, _) => {
                let opened = node.open_pushing(to, units, push, delay, &mut NodeGround { engine: &mut runner.engine });
                match opened {
                    Ok(id) => ("ch.opening", short_id(&id), true),
                    Err(refusal) => refused(runner, refusal),
                }
            }
            (false, None) => ("ch.not_connected", hex(&to.0[..4]), false),
            (false, Some(address)) => {
                runner.add_address(TCP_LINK, address.clone());
                self.waiting.retain(|waiting| waiting.key != to);
                let until_ms = unix_ms() + REACH_WAIT_MS;
                self.waiting.push(Waiting { key: to, address: address.clone(), amount: units, push, delay, until_ms });
                ("ch.connecting", address, true)
            }
        }
    }

    /// Opens each channel waiting on a node that has now said hello, and gives up on the ones
    /// whose node did not answer in time.
    pub(super) fn open_reached(&mut self, runner: &mut Runner, now: u64) {
        let Some(node) = self.node.as_mut().filter(|_| !self.waiting.is_empty()) else {
            return;
        };
        let mut said = Vec::new();
        let mut ground = NodeGround { engine: &mut runner.engine };
        let heard = node.peers();
        self.waiting.retain(|waiting| {
            if heard.iter().any(|(key, _)| *key == waiting.key) {
                let opened = node.open_pushing(waiting.key, waiting.amount, waiting.push, waiting.delay, &mut ground);
                said.push(match opened {
                    Ok(id) => ("ch.said_reached", short_id(&id)),
                    Err(refusal) => (refusal_key(refusal), String::new()),
                });
                return false;
            }
            if now >= waiting.until_ms {
                said.push(("ch.said_unreached", waiting.address.clone()));
                return false;
            }
            true
        });
        for (key, detail) in said {
            self.tell(key, detail);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "02aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    /// One field takes every way a person copies an address, and says which part is wrong.
    #[test]
    fn a_channel_address_reads_with_or_without_its_port() {
        let at = |text: &str| read_channel_address(text).map(|(_, address)| address);
        assert_eq!(at(&format!("{KEY}@127.0.0.1:7123")), Ok(Some("127.0.0.1:7123".into())));
        assert_eq!(at(&format!(" {KEY}@example.org ")), Ok(Some(format!("example.org:{USUAL_PORT}"))));
        assert_eq!(at(&format!("{KEY}@::1")), Ok(Some(format!("[::1]:{USUAL_PORT}"))));
        assert_eq!(at(&format!("{KEY}@[::1]:7001")), Ok(Some("[::1]:7001".into())));
        assert_eq!(at(KEY), Ok(None));
        assert_eq!(at(&format!("{KEY}@")), Err("ch.bad_address"));
        assert_eq!(at(&format!("{KEY}@host:port")), Err("ch.bad_address"));
        assert_eq!(at("02abcd@127.0.0.1:7000"), Err("ch.bad_key"));
    }

    #[test]
    fn the_wait_is_the_usual_one_unless_typed_within_bounds() {
        assert_eq!(read_delay(""), Some(DELAY));
        assert_eq!(read_delay("200"), Some(200));
        assert_eq!(read_delay("1"), None);
        assert_eq!(read_delay("many"), None);
    }
}
