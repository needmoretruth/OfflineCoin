//! A payment sent from here ends in a notice of how it went, so "sending" never outlives it.
use super::{ChannelDesk, ChannelEvent};
use super::super::host::Host;

/// How a payment ended, as the channel node said it.
const ENDINGS: [&str; 2] = ["ch.said_paid", "ch.said_pay_failed"];

/// The first payment ending said after event `from`. Events run newest first.
pub(super) fn ending_after(events: &[ChannelEvent], from: u64) -> Option<&ChannelEvent> {
    events.iter().rev().find(|event| event.number > from && ENDINGS.contains(&event.key))
}

impl ChannelDesk {
    /// Notes that a payment just left, so its ending can replace the notice that it is on its way.
    pub(super) fn paying_now(&mut self) {
        self.paying_from = Some(self.number);
    }

    fn payment_ended(&mut self) -> Option<(&'static str, String, bool)> {
        let from = self.paying_from?;
        let events: Vec<ChannelEvent> = self.events.iter().cloned().collect();
        let ended = ending_after(&events, from)?;
        self.paying_from = None;
        Some((ended.key, ended.detail.clone(), ended.key == ENDINGS[0]))
    }
}

impl Host {
    /// How the payment sent last ended, once it has, and only once.
    pub fn payment_ended(&self) -> Option<(&'static str, String, bool)> {
        self.state.channels.lock().ok()?.payment_ended()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(number: u64, key: &'static str) -> ChannelEvent {
        ChannelEvent { number, key, detail: String::new() }
    }

    /// Only an ending said after the payment left counts: an older one belongs to another payment.
    #[test]
    fn the_ending_of_a_payment_is_the_first_one_said_after_it_left() {
        let events = [event(9, "ch.said_received"), event(8, "ch.said_pay_failed"), event(7, "ch.said_paid"), event(5, "ch.said_paid")];
        assert_eq!(ending_after(&events, 6).map(|event| event.number), Some(7));
        assert_eq!(ending_after(&events, 7).map(|event| event.key), Some("ch.said_pay_failed"));
        assert!(ending_after(&events, 8).is_none());
    }
}
