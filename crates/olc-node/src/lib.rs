//! The node. [`Engine`] holds the logic and never touches a device. [`Link`] is the socket for
//! transports, and [`Runner`] connects the two. A new transport is a new `Link`.
#![no_std]
extern crate alloc;

pub mod bluetooth;
mod channel;
pub mod bump;
pub mod dial;
pub mod engine;
mod event;
pub mod fragment;
pub mod frame;
pub mod hello;
pub mod lan;
pub mod light;
pub mod line;
pub mod locks;
mod pool;
pub mod medium;
pub mod message;
pub mod neighbours;
pub mod network;
mod refused;
pub mod runner;
pub mod shared;
mod serve;
mod snapshot;
mod spend;
mod sweep;
mod sync;
mod work;

pub use engine::{Engine, PeerId, MOST_NOTE, MOST_PEERS, MOST_QUEUED};
pub use event::{Event, Outgoing, Status};
pub use hello::{Card, Hello, Keeps};
pub use light::{Light, LightStatus, MOST_HEADERS_ON_A_BOARD};
pub use medium::{Medium, Rank};
pub use message::{Heights, Message};
pub use neighbours::Neighbour;
pub use pool::{MOST_DATA, MOST_POOL_BYTES};
pub use runner::{Link, News, Runner, Target};
