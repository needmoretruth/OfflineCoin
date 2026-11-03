//! Pictures of the chain a person mines on: the latest blocks and who found each, the branch that
//! raced them, each miner's share, this computer's speed and the wait for the next block. All of
//! them read [`ChainView`] from `snapshot.node.chain` and draw only what it holds.
//!
//! Every widget takes a [`Look`]: the tokens, the reader's words, whether motion is off and the
//! time now. Build it once per frame:
//!
//! ```ignore
//! let look = Look { t: &t, voice, still: settings.reduce_motion || desktop_reduces_motion(),
//!                   now_ms: unix_ms(), ticker: &ticker };
//! let chain = &snapshot.node.chain;
//! chain::block_strip(ui, &look, &chain::Strip::of(chain));   // Home, Explorer
//! chain::race_bars(ui, &look, &chain.shares);                 // Mining
//! chain::hashrate_line(ui, &look, chain, 120.0);              // Home mining card, Mining
//! chain::next_block_bar(ui, &look, chain);                    // Home mining card, Mining
//! ```
//!
//! - [`block_strip`] draws blocks lit from above, newest on the left: height above, miner below,
//!   how long ago on the face. "You" takes the one accent, everyone else a gray. A new block slides
//!   in from the left in 400 ms unless `still`. A [`Strip::branch`] draws a second, offset row under
//!   the heights it shares with the first: a race still open, or the blocks a reorg dropped. The Lab
//!   passes its own blocks and branch the same way.
//! - [`race_bars`] draws one bar per miner, most blocks first, with percent and block count.
//! - [`hashrate_line`] draws this computer's speed over ten minutes: a thin line on a soft fill and
//!   the number now.
//! - [`next_block_bar`] fills from the last block toward the expected wait.
//!
//! Each returns the [`egui::Response`] of the space it took. Blocks and bars carry their words as
//! accessible labels, so a screen test can find them.
mod pace;
mod race;
mod strip;

pub use pace::{hashrate_line, next_block_bar};
pub use race::race_bars;
pub use strip::{block_strip, BranchKind, Strip};
pub(crate) use strip::{depth, draw_box};

use super::tokens::Tokens;
use crate::core::{ChainView, Miner};
use crate::i18n::{self, Voice};
use egui::Color32;

/// What every chain widget needs besides its data.
pub struct Look<'a> {
    pub t: &'a Tokens,
    pub voice: Voice,
    /// The app's "keep things still" setting or the desktop's: no slide, no growing bar.
    pub still: bool,
    /// Unix time in milliseconds, for "8 s ago" and the next block's wait.
    pub now_ms: u64,
    /// The coin's short name, for rewards.
    pub ticker: &'a str,
}

/// How far `top` moves toward `bottom`, from 0 to 1.
pub(crate) fn mix(top: Color32, bottom: Color32, part: f32) -> Color32 {
    let channel = |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * part).round() as u8;
    Color32::from_rgba_premultiplied(
        channel(top.r(), bottom.r()),
        channel(top.g(), bottom.g()),
        channel(top.b(), bottom.b()),
        channel(top.a(), bottom.a()),
    )
}

/// The faces of one block: lit top, front from light to shade, the side in shadow, and its ink.
#[derive(Clone, Copy)]
pub(crate) struct Faces {
    pub top: Color32,
    pub front_top: Color32,
    pub front_bottom: Color32,
    pub side: Color32,
    pub ink: Color32,
    pub label: Color32,
}

impl Faces {
    /// The two ends of a filled bar in the block's colour. On a light page the lit top is nearly
    /// the track's own white, so a bar there runs from the front to the side in shadow instead.
    pub(crate) fn bar(&self, t: &Tokens) -> (Color32, Color32) {
        if t.dark {
            (self.top, self.front_bottom)
        } else {
            (self.front_bottom, self.side)
        }
    }
}

/// "You" in the accent, everyone else in one gray: color only says whose it is.
pub(crate) fn faces(t: &Tokens, you: bool) -> Faces {
    let base = if you { t.info } else { gray(t) };
    let ink = match (you, t.dark) {
        (true, true) => t.back_mid,
        (true, false) => t.card_top,
        (false, _) => t.text,
    };
    faces_in(t, base, ink, if you { t.info } else { t.soft })
}

/// The gray of every block that is not the person's.
pub(crate) fn gray(t: &Tokens) -> Color32 {
    mix(t.card_top, t.soft, if t.dark { 0.30 } else { 0.24 })
}

/// The same light and shadow on any base colour, so a block in a state colour, such as the
/// attacker's red in the Lab, reads as the same kind of block as the strip's.
pub(crate) fn faces_in(t: &Tokens, base: Color32, ink: Color32, label: Color32) -> Faces {
    let (light, shade) = match t.dark {
        true => (t.text, t.back_floor),
        false => (t.card_top, t.text),
    };
    Faces {
        top: mix(base, light, if t.dark { 0.32 } else { 0.55 }),
        front_top: mix(base, light, 0.10),
        front_bottom: base,
        side: mix(base, shade, if t.dark { 0.45 } else { 0.28 }),
        ink,
        label,
    }
}

/// A miner as the reader names it.
pub fn miner_name(voice: Voice, miner: &Miner) -> String {
    match miner {
        Miner::You => i18n::text(voice, "chain.you").to_string(),
        Miner::Here(name) | Miner::Board(name) => name.clone(),
        Miner::Away(address) => super::format::short_address(address),
        Miner::Unknown => i18n::text(voice, "chain.unknown").to_string(),
    }
}

/// How long ago, short enough for the face of a block.
pub(crate) fn ago(voice: Voice, now_ms: u64, then_ms: u64) -> String {
    let seconds = now_ms.saturating_sub(then_ms) / 1_000;
    let (key, count) = match seconds {
        0..=119 => ("chain.secs_ago", seconds),
        120..=7_199 => ("chain.mins_ago", seconds / 60),
        _ => ("chain.hours_ago", seconds / 3_600),
    };
    i18n::fill(i18n::text(voice, key), &[&super::format::number(count.into())])
}

/// A wait in milliseconds as the reader says it, to the second below two minutes.
pub(crate) fn wait(voice: Voice, ms: u64) -> String {
    super::format::seconds(voice, ms.div_ceil(1_000).max(1))
}

/// Whether the view has anything to draw yet.
pub fn has_blocks(chain: &ChainView) -> bool {
    !chain.latest.is_empty()
}
