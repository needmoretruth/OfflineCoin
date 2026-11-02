//! Running boards from afar, over the same links that carry the chain. Anyone may listen and ask
//! what a board shows. Changing anything takes a signature from the control key.
#![no_std]
extern crate alloc;

pub mod beacon;
pub mod bootlog;
pub mod defaults;
pub mod gate;
pub mod line;
pub mod packet;
pub mod router;
pub mod sending;
pub mod update;
pub mod wear;

pub use beacon::{sign_beacon, SignedBeacon};
pub use gate::{allowed_remotely, sign, Gate, REMOTE_WORDS};
pub use packet::{Action, Beacon, BoardId, Body, OldBeacon, Order, Packet, Read, Refusal, Signed};
pub use router::{Forward, Router, Verdict};
pub use update::{ImageSlot, Update, CHUNK, PAGE, PIECE};
