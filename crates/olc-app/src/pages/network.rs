//! The whole network as a Bitcoin user reads it: its speed from the latest blocks, the average
//! over every block kept, this computer's share of that speed, and who found the latest blocks.
//! The Mining page shows it in full; Home shows the same figures under its mining card.
use super::Cx;
use crate::core::{unix_ms, ChainView, FEWEST_BLOCKS};
use crate::ui::chain::{self, Look};
use crate::ui::explain;
use crate::ui::tokens;
use crate::ui::widgets::{card, section, strong_text, Columns};
use crate::ui::format;
use egui::{Label, RichText, Ui};

/// This computer's part of the network's speed, in whole percent: what it measures over the
/// network's speed, which never reads below it. `None` while too few blocks give the network's
/// speed. Who found the blocks is the breakdown's to say, in its own words.
pub fn share_percent(chain: &ChainView) -> Option<u64> {
    chain.network?;
    let percent = (u128::from(chain.hashrate_here) * 100).checked_div(u128::from(chain.hashrate_all))?;
    Some(u64::try_from(percent).unwrap_or(100).min(100))
}

/// The network's speed as every page writes it, or that it is not known yet.
pub fn network_rate(cx: &Cx) -> String {
    let chain = &cx.snap.node.chain;
    match chain.network {
        Some(_) => format::rate(cx.voice(), chain.hashrate_all),
        None => cx.say("rate.unknown").to_string(),
    }
}

/// How long a speed reading spans, in whole seconds or minutes.
fn span_words(cx: &Cx, span_ms: u64) -> String {
    format::seconds(cx.voice(), span_ms.div_ceil(1_000))
}

/// The figures side by side, each a quiet name over its value; while too few blocks are kept to
/// read the network's speed from, one line that says so instead.
pub fn figures(ui: &mut Ui, cx: &Cx) {
    let t = cx.t;
    let voice = cx.voice();
    let chain = &cx.snap.node.chain;
    let (Some(reading), Some(share)) = (chain.network, share_percent(chain)) else {
        let needed = format::number((FEWEST_BLOCKS + 1) as u128);
        let line = cx.fill("net.too_few", &[&needed, &format::number(chain.latest.len() as u128)]);
        ui.add(Label::new(RichText::new(line).size(tokens::META).color(t.muted)).wrap());
        return;
    };
    // The speed now says under it how far back it reads, which is how long a change takes to show.
    let span = cx.fill("net.now_span", &[&span_words(cx, reading.span_ms)]);
    let mut shown = vec![(cx.say("net.now").to_string(), format::rate(voice, chain.hashrate_all), Some(span))];
    // Until more blocks are kept than the speed now reads, the average reads the same blocks and
    // would only say the same number twice.
    let blocks = chain.latest.len().saturating_sub(1);
    if blocks > reading.blocks && chain.hashrate_average > 0 {
        shown.push((cx.fill("net.average", &[&format::number(blocks as u128)]), format::rate(voice, chain.hashrate_average), None));
    }
    shown.push((cx.say("net.share").to_string(), format!("{share}%"), None));
    Columns::new(ui, &[1.0; 3]).row(ui, |ui, index| {
        let Some((name, value, under)) = shown.get(index) else {
            return;
        };
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            ui.add(Label::new(RichText::new(name).size(tokens::META).color(t.muted)).selectable(false).wrap());
            ui.label(strong_text(value));
            if let Some(under) = under {
                ui.add(Label::new(RichText::new(under).size(tokens::META).color(t.muted)).selectable(false).wrap());
            }
        });
    });
}

/// The card on the Mining page: the figures, then who found the latest blocks and the next one's wait.
pub fn card_of(ui: &mut Ui, cx: &Cx, still: bool) {
    let t = cx.t;
    card(ui, t, |ui| {
        section(ui, cx.say("net.title"));
        figures(ui, cx);
        let chain = &cx.snap.node.chain;
        if let Some(reading) = chain.network {
            let span = span_words(cx, reading.span_ms);
            explain::short(ui, t, &cx.fill("net.about", &[&span, &format::number(reading.blocks as u128), &span]));
        }
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
    use crate::core::{Miner, Reading, Share};

    /// This computer's share is its measured speed over the network's, whatever share of the
    /// blocks it found: a lucky run of blocks does not make it more, and the network's speed it
    /// is read against never falls below it. With too few blocks there is no share to give.
    #[test]
    fn this_computers_share_is_its_speed_over_the_networks() {
        let reading = Some(Reading { blocks: 15, span_ms: 30_000, rate: 40_000 });
        let lucky = ChainView {
            hashrate_here: 10_000,
            hashrate_all: 40_000,
            network: reading,
            shares: vec![Share { miner: Miner::You, blocks: 30, hashrate: Some(10_000), workers: Vec::new() }],
            ..ChainView::default()
        };
        assert_eq!(share_percent(&lucky), Some(25), "every block was this computer's, its speed is a quarter");
        let alone = ChainView { hashrate_here: 30_000, hashrate_all: 30_000, network: reading, ..ChainView::default() };
        assert_eq!(share_percent(&alone), Some(100));
        let early = ChainView { hashrate_here: 30_000, hashrate_all: 30_000, network: None, ..ChainView::default() };
        assert_eq!(share_percent(&early), None);
        assert_eq!(share_percent(&ChainView::default()), None);
    }
}
