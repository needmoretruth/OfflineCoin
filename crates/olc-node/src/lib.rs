//! The node. [`Engine`] holds the logic and never touches a device. [`Link`] is the socket for
//! transports, and [`Runner`] connects the two. A new transport is a new `Link`.
#![no_std]
extern crate alloc;

pub mod bluetooth;
pub mod dial;
pub mod engine;
mod event;
pub mod fragment;
pub mod frame;
pub mod hello;
pub mod line;
pub mod medium;
pub mod message;
pub mod neighbours;
pub mod network;
mod refused;
pub mod runner;
mod snapshot;
mod spend;
mod sync;
mod work;

pub use engine::{Engine, PeerId, MOST_DATA, MOST_NOTE};
pub use event::{Event, Outgoing, Status};
pub use hello::{Card, Hello};
pub use medium::{Medium, Rank};
pub use message::{Heights, Message};
pub use neighbours::Neighbour;
pub use runner::{Link, News, Runner, Target};
