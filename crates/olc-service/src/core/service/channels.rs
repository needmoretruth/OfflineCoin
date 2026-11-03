//! Orders about payment channels, carried out by the node's channel desk.
use super::super::commands::ChannelOrder;
use super::Service;

impl Service {
    /// Orders about payment channels, which also end in a notice.
    pub(super) fn channel_order(&mut self, order: ChannelOrder) {
        let (key, detail, good) = self.host.carry_channel(order);
        // A quote is read off the snapshot, and a notice would only cover the slip.
        if !key.is_empty() {
            self.say(key, detail, good);
        }
    }

    /// Puts how a payment ended in place of the notice that it is on its way.
    pub(super) fn follow_payment(&mut self) {
        if let Some((key, detail, good)) = self.host.payment_ended() {
            self.say(key, detail, good);
        }
    }
}
