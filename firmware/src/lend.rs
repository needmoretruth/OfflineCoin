//! Pool work a computer lends a board it reaches only through another board. The board on the
//! cable passes it one hop on; the board it is for grinds it and sends the shares back to the
//! computer. Both answer at once, so the computer can tell which of them stays silent. What to do
//! with each packet is decided in `olc_control::lend`, so the computer's tests run the same rules.
use crate::commands::Work;
use crate::node::Node;
use alloc::vec::Vec;
use olc_control::lend::{handle, receipt, Handling, LENT_FOR_MS};
use olc_control::{Body, Packet};
use olc_core::BlockHeader;
use olc_node::{Message, Outgoing};

impl Node {
    /// Lent work that reached this board: passed on to the board it is for, or taken.
    pub fn lent(&mut self, packet: &Packet, now: u64) {
        let me = self.runner.engine.address();
        let handling = handle(packet, &me, &self.control.gate, &self.control.heard);
        // The computer hears that the work went on, or was taken, so it can name a silent board.
        let answer = receipt(packet, &handling, &me);
        match handling {
            Handling::Pass(peer, passed) => {
                self.runner.send_now(Outgoing::To(peer, Message::Control(passed.encode())));
            }
            Handling::Take(Some((header, difficulty))) => {
                let Some(header) = BlockHeader::decode(&header) else {
                    return;
                };
                let found = self.work.take().map(|work| work.found).unwrap_or_default();
                self.work = Some(Work {
                    header,
                    difficulty,
                    found,
                    lender: Some((packet.from, now + LENT_FOR_MS)),
                });
            }
            // A stop ends lent work only: work given by cable or by order stays.
            Handling::Take(None) => {
                if self.work.as_ref().is_some_and(|work| work.lender.is_some()) {
                    self.work = None;
                }
            }
            Handling::Ignore => {}
        }
        if let Some(body) = answer {
            self.start_packet(Some(packet.from), body);
        }
    }

    /// Sends what lent work found to the computer that lent it, and ends lent work that nothing
    /// renewed, so a board whose computer went away mines for itself again.
    pub fn serve_lent(&mut self, now: u64) {
        let Some(work) = &mut self.work else {
            return;
        };
        let Some((lender, until)) = work.lender else {
            return;
        };
        if now >= until {
            self.work = None;
            return;
        }
        if work.found.is_empty() {
            return;
        }
        let headers: Vec<Vec<u8>> = work.found.drain(..).map(|header| header.encode().to_vec()).collect();
        let worker = self.runner.engine.address();
        self.start_packet(Some(lender), Body::Shares { worker, headers });
    }
}
