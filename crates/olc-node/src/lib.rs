//! The node. [`Engine`] holds the logic and never touches a device. [`Link`] is the socket for
//! transports, and [`Runner`] connects the two. A new transport is a new `Link`.
#![no_std]
extern crate alloc;

pub mod engine;
mod event;
pub mod fragment;
pub mod frame;
pub mod hello;
pub mod light;
pub mod message;
pub mod network;
mod refused;
pub mod runner;
mod serve;
mod snapshot;
mod spend;
mod sync;
mod work;

pub use engine::{Engine, PeerId, MOST_DATA, MOST_NOTE};
pub use event::{Event, Outgoing, Status};
pub use hello::{Card, Hello};
pub use light::{Light, LightStatus, MOST_HEADERS_ON_A_BOARD};
pub use message::{Heights, Message};
pub use runner::{Link, Runner, Target};
