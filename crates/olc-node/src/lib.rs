//! The node. [`Engine`] holds the logic and never touches a device. [`Link`] is the socket for
//! transports, and [`Runner`] connects the two. A new transport is a new `Link`.
#![no_std]
extern crate alloc;

pub mod engine;
mod event;
pub mod fragment;
pub mod frame;
pub mod message;
pub mod network;
mod refused;
pub mod runner;
mod sync;

pub use engine::{Engine, PeerId};
pub use event::{Event, Outgoing, Status};
pub use message::Message;
pub use runner::{Link, Runner, Target};
