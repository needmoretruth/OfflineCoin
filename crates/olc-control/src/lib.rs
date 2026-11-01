//! Running boards from afar, over the same links that carry the chain. Anyone may listen and ask
//! what a board shows. Changing anything takes a signature from the control key.
#![no_std]
extern crate alloc;

pub mod gate;
pub mod packet;
pub mod router;
pub mod update;

pub use gate::{REMOTE_WORDS, allowed_remotely, sign, Gate};
pub use packet::{Action, Beacon, BoardId, Body, Order, Packet, Read, Refusal, Signed};
pub use router::{Forward, Router, Verdict};
pub use update::{ImageSlot, Update, CHUNK, PAGE};
