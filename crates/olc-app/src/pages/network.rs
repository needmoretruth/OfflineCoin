//! The whole network as a Bitcoin user reads it: its speed from the last blocks, the average over
//! every block kept, this computer's share of it, and who found the latest blocks. The Mining page
//! shows it in full; Home shows the same three figures under its mining card.
use super::Cx;
use crate::core::{unix_ms, ChainView, Miner};
use crate::ui::chain::{self, Look};
use crate::ui::explain;
use crate::ui::tokens;
use crate::ui::widgets::{card, section, strong_text, Columns};
use crate::ui::format;
use egui::{Label, RichText, Ui};

/// This computer's part of the network's speed, in whole percent. The network is this computer,
/// the boards heard on the same chain, and the speed nobody here accounts for: what the blocks took
/// beyond the speeds heard, counted only once a miner nobody here knows found one of the latest
/// blocks. So a network of this computer alone reads 100 %, not whatever a lucky run of blocks says.
pub fn share_percent(chain: &ChainView) -> Option<u64> {
    let here = chain.hashrate_here;
    let stranger = |miner: &Miner| matches!(miner, Miner::Away(_) | Miner::Pool(_) | Miner::Unknown);
    let boards: u64 = chain.shares.iter().filter(|share| matches!(share.miner, Miner::Board(_))).filter_map(|share| share.hashrate).sum();
    let strangers_found = chain.shares.iter().any(|share| stranger(&share.miner) && share.blocks > 0);
    let unknown = if strangers_found { chain.hashrate_all.saturating_sub(here + boards) } else { 0 };
    let percent = (u128::from(here) * 100).checked_div(u128::from(here + boards + unknown))?;
    Some(u64::try_from(percent).unwrap_or(100))
}

/// The three figures side by side, each a quiet name over its value.
pub fn figures(ui: &mut Ui, cx: &Cx) {
    let t = cx.t;
    let voice = cx.voice();
    let chain = &cx.snap.node.chain;
    let unknown = cx.say("rate.unknown").to_string();
    let known = |rate: u64| if rate == 0 { unknown.clone() } else { format::rate(voice, rate) };
    let blocks = chain.latest.len().saturating_sub(1);
    let shown = [
        (cx.say("net.now").to_string(), known(chain.hashrate_all)),
        (cx.fill("net.average", &[&format::number(blocks as u128)]), known(chain.hashrate_average)),
        (cx.say("net.share").to_string(), share_percent(chain).map_or(unknown.clone(), |percent| format!("{percent}%"))),
    ];
    Columns::new(ui, &[1.0; 3]).row(ui, |ui, index| {
        let Some((name, value)) = shown.get(index) else {
            return;
        };
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            ui.add(Label::new(RichText::new(name).size(tokens::META).color(t.muted)).selectable(false).wrap());
            ui.label(strong_text(value));
        });
    });
}

/// The card on the Mining page: the figures, then who found the latest blocks and the next one's wait.
pub fn card_of(ui: &mut Ui, cx: &Cx, still: bool) {
    let t = cx.t;
    card(ui, t, |ui| {
        section(ui, cx.say("net.title"));
        figures(ui, cx);
        explain::short(ui, t, cx.say("net.about"));
        ui.add_space(tokens::GAP_WIDE);
        ui.label(RichText::new(cx.say("mining.race")).color(t.text));
        let ticker = cx.ticker().to_string();
        let look = Look { t, voice: cx.voice(), still, now_ms: unix_ms(), ticker: &ticker };
        chain::race_bars(ui, &look, &cx.snap.node.chain.shares);
        ui.add_space(tokens::GAP);
        chain::next_block_bar(ui, &look, &cx.snap.node.chain);
        let labels = (cx.say("explain.details"), cx.say("explain.hide"));
        explain::details(ui, t, "mining-race", labels, |ui| {
            ui.label(cx.say("mining.race_about"));
            ui.add_space(tokens::GAP);
            explain::analogy(ui, t, cx.say("mining.race_analogy"));
            ui.add_space(tokens::GAP);
            ui.label(cx.say("mining.race_more"));
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::Share;

    fn share(miner: Miner, blocks: u32, hashrate: Option<u64>) -> Share {
        Share { miner, blocks, hashrate, workers: Vec::new() }
    }

    #[test]
    fn this_computers_share_counts_boards_and_strangers_only_when_they_are_there() {
        // Every block is this computer's: a lucky estimate above its speed still reads 100 %.
        let alone = ChainView {
            hashrate_here: 30_000,
            hashrate_all: 32_600,
            shares: vec![share(Miner::You, 30, Some(10_000)), share(Miner::OwnPool, 10, None)],
            ..ChainView::default()
        };
        assert_eq!(share_percent(&alone), Some(100));
        // Three boards of 15,000 beside 5,000 here.
        let boards = ChainView {
            hashrate_here: 5_000,
            hashrate_all: 50_000,
            shares: vec![share(Miner::Board("보드원".into()), 20, Some(45_000)), share(Miner::You, 3, Some(5_000))],
            ..ChainView::default()
        };
        assert_eq!(share_percent(&boards), Some(10));
        // A stranger found blocks: what the blocks took beyond the speeds heard is theirs.
        let stranger = ChainView {
            hashrate_here: 10_000,
            hashrate_all: 40_000,
            shares: vec![share(Miner::Away("olcx".into()), 30, None), share(Miner::You, 10, Some(10_000))],
            ..ChainView::default()
        };
        assert_eq!(share_percent(&stranger), Some(25));
        assert_eq!(share_percent(&ChainView::default()), None);
    }
}
