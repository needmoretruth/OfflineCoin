//! Decides which orders a board obeys.
use crate::packet::{Action, BoardId, Order, Refusal, Signed};
use alloc::vec::Vec;
use olc_crypto::{hash, verify, Hash, PublicKey};

/// Keeps a control signature from ever meaning anything else.
const DOMAIN: &[u8] = b"OfflineCoin control order 1\n";

/// What a signed line may do from afar. The cable can do everything, but from afar a mistake
/// could cut the only way to the board. `key` is never here, so no radio line reaches a secret.
pub const REMOTE_WORDS: [&str; 28] = [
    "status", "tx", "limit", "mine", "offline", "pay", "pool", "view", "lang", "name", "logo",
    "boots", "smooth", "bright", "clock", "full", "network", "power", "ignore", "keys", "mineto",
    "keep", "merge", "note", "minenote", "snapshot", "coin", "work",
];

pub fn allowed_remotely(line: &str) -> bool {
    let word = line.split_whitespace().next().unwrap_or_default();
    REMOTE_WORDS.contains(&word)
}

fn digest(order: &Order) -> Hash {
    let mut bytes = Vec::from(DOMAIN);
    bytes.extend(postcard::to_allocvec(order).unwrap_or_default());
    hash(&bytes)
}

pub fn sign(secret: &[u8; 32], order: Order) -> Option<Signed> {
    let signature = olc_crypto::sign(secret, &digest(&order))?.to_vec();
    Some(Signed { order, signature })
}

pub struct Gate {
    me: BoardId,
    key: Option<PublicKey>,
    counter: u64,
}

impl Gate {
    pub fn new(me: BoardId, key: Option<PublicKey>, counter: u64) -> Self {
        Self { me, key, counter }
    }

    pub fn counter(&self) -> u64 {
        self.counter
    }

    pub fn has_key(&self) -> bool {
        self.key.is_some()
    }

    /// Checks one order and returns what it asks. The caller saves the new counter first, so a
    /// power cut cannot make an order valid twice.
    pub fn admit(&mut self, signed: &Signed) -> Result<Action, Refusal> {
        let key = self.key.ok_or(Refusal::NoKey)?;
        let order = &signed.order;
        if order.board != self.me {
            return Err(Refusal::WrongBoard);
        }
        if order.counter <= self.counter {
            return Err(Refusal::Replayed);
        }
        if !verify(&key, &digest(order), &signed.signature) {
            return Err(Refusal::BadSignature);
        }
        if let Action::Line(line) = &order.action {
            if !allowed_remotely(line) {
                return Err(Refusal::CableOnly);
            }
        }
        self.counter = order.counter;
        Ok(order.action.clone())
    }
}

#[cfg(test)]
mod key_tests {
    use super::*;

    #[test]
    fn no_radio_line_reaches_a_secret_key() {
        assert!(!REMOTE_WORDS.contains(&"key"));
        for line in ["key export 0", "key  new", "key", " key import 00", "KEY export 0"] {
            assert!(!allowed_remotely(line), "{line}");
        }
        assert!(allowed_remotely("keys"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;

    const SECRET: [u8; 32] = [7; 32];
    const ME: BoardId = [1; 6];

    fn gate() -> Gate {
        Gate::new(ME, olc_crypto::public_key(&SECRET), 5)
    }

    fn order(board: BoardId, counter: u64, line: &str) -> Order {
        Order {
            board,
            counter,
            action: Action::Line(line.to_string()),
        }
    }

    #[test]
    fn a_signed_order_is_obeyed_once() {
        let mut gate = gate();
        let signed = sign(&SECRET, order(ME, 6, "limit 100")).unwrap();
        assert_eq!(
            gate.admit(&signed),
            Ok(Action::Line("limit 100".to_string()))
        );
        assert_eq!(gate.counter(), 6);
        assert_eq!(gate.admit(&signed), Err(Refusal::Replayed));
    }

    #[test]
    fn an_old_counter_is_refused() {
        let signed = sign(&SECRET, order(ME, 5, "limit 100")).unwrap();
        assert_eq!(gate().admit(&signed), Err(Refusal::Replayed));
    }

    #[test]
    fn no_signature_or_a_stranger_is_refused() {
        let unsigned = Signed {
            order: order(ME, 6, "limit 100"),
            signature: Vec::new(),
        };
        assert_eq!(gate().admit(&unsigned), Err(Refusal::BadSignature));
        let stranger = sign(&[9; 32], order(ME, 6, "limit 100")).unwrap();
        assert_eq!(gate().admit(&stranger), Err(Refusal::BadSignature));
    }

    #[test]
    fn a_changed_order_loses_its_signature() {
        let mut signed = sign(&SECRET, order(ME, 6, "limit 100")).unwrap();
        signed.order.action = Action::Line("limit 999".to_string());
        assert_eq!(gate().admit(&signed), Err(Refusal::BadSignature));
    }

    #[test]
    fn an_order_for_another_board_is_refused() {
        let signed = sign(&SECRET, order([2; 6], 6, "limit 100")).unwrap();
        assert_eq!(gate().admit(&signed), Err(Refusal::WrongBoard));
    }

    #[test]
    fn a_board_without_a_key_obeys_nobody() {
        let mut gate = Gate::new(ME, None, 0);
        let signed = sign(&SECRET, order(ME, 6, "limit 100")).unwrap();
        assert_eq!(gate.admit(&signed), Err(Refusal::NoKey));
    }

    #[test]
    fn some_lines_need_the_cable() {
        for line in ["espnow off", "trust 00", "wifi off", "dial off"] {
            let signed = sign(&SECRET, order(ME, 6, line)).unwrap();
            assert_eq!(gate().admit(&signed), Err(Refusal::CableOnly), "{line}");
        }
    }
}
