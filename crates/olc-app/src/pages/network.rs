//! The whole network as a Bitcoin user reads it: its speed from the latest blocks, the average
//! over every block kept, this computer's share of that speed, and who found the latest blocks.
//! The Mining page shows it in full; Home shows the speed now and the share under its mining card.
use super::Cx;
use crate::core::{unix_ms, FEWEST_BLOCKS};
use crate::ui::chain::{self, Look};
use crate::ui::explain;
use crate::ui::tokens;
use crate::ui::widgets::{card, section, strong_text, Columns};
use crate::ui::format;
use egui::{Label, RichText, Ui};

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
/// read the network's speed from, one line that says so instead. The long average follows a change
/// of speed only after its whole span, so beside this computer's speed now it can read as a
/// contradiction: only the Mining card shows it (`with_average`), saying how late it follows.
pub fn figures(ui: &mut Ui, cx: &Cx, with_average: bool) {
    let t = cx.t;
    let voice = cx.voice();
    let chain = &cx.snap.node.chain;
    let (Some(reading), Some((share, error))) = (chain.network, chain.share()) else {
        let needed = format::number((FEWEST_BLOCKS + 1) as u128);
        let line = cx.fill("net.too_few", &[&needed, &format::number(chain.latest.len() as u128)]);
        ui.add(Label::new(RichText::new(line).size(tokens::META).color(t.muted)).wrap());
        return;
    };
    // The speed now says under it whether it is measured or how far back it reads the others'
    // blocks, which is how long a change of theirs takes to show; the share, how far off it may be.
    let span = match reading.others {
        0 => cx.say("net.now_measured").to_string(),
        _ => cx.fill("net.now_span", &[&span_words(cx, reading.span_ms)]),
    };
    let off = (error > 0).then(|| cx.fill("net.share_error", &[&error.to_string()]));
    let mut shown = vec![
        (cx.say("net.now").to_string(), format::rate(voice, chain.hashrate_all), Some(span)),
        (cx.say("net.share").to_string(), format!("{share}%"), off),
    ];
    // Until more blocks are kept than the speed now reads, the average reads the same blocks and
    // would only say the same number twice.
    let blocks = chain.latest.len().saturating_sub(1);
    if with_average && blocks > reading.blocks && chain.hashrate_average > 0 {
        // The newest block first: the average spans from the oldest kept to the newest.
        let spans = chain.latest.first().zip(chain.latest.last()).map_or(0, |(newest, oldest)| newest.time_ms.saturating_sub(oldest.time_ms));
        let late = cx.fill("net.average_late", &[&span_words(cx, spans)]);
        shown.push((cx.fill("net.average", &[&format::number(blocks as u128)]), format::rate(voice, chain.hashrate_average), Some(late)));
    }
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
        figures(ui, cx, true);
        let chain = &cx.snap.node.chain;
        if let Some(reading) = chain.network {
            let span = span_words(cx, reading.span_ms);
            let about = match reading.others {
                0 => cx.fill("net.about_measured", &[&span, &format::number(reading.blocks as u128)]),
                others => cx.fill("net.about", &[&span, &format::number(others as u128), &span]),
            };
            explain::short(ui, t, &about);
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
    use crate::core::{ChainView, Miner, Reading, Share};

    /// This computer's share is its measured speed over the network's, whatever share of the
    /// blocks it found: a lucky run of blocks does not make it more, and the network's speed it
    /// is read against never falls below it. With too few blocks there is no share to give.
    #[test]
    fn this_computers_share_is_its_speed_over_the_networks() {
        let reading = Some(Reading { blocks: 15, span_ms: 30_000, rate: 40_000, others: 5, others_rate: 30_000 });
        let lucky = ChainView {
            hashrate_here: 10_000,
            hashrate_all: 40_000,
            network: reading,
            shares: vec![Share { miner: Miner::You, blocks: 30, hashrate: Some(10_000), workers: Vec::new() }],
            ..ChainView::default()
        };
        // Thirty blocks were this computer's, yet its speed is a quarter; the others' three
        // quarters, read from five blocks, may move it by 17 points.
        assert_eq!(lucky.share(), Some((25, 17)));
        let measured = reading.map(|reading| Reading { others: 0, others_rate: 0, ..reading });
        let alone = ChainView { hashrate_here: 30_000, hashrate_all: 30_000, network: measured, ..ChainView::default() };
        assert_eq!(alone.share(), Some((100, 0)), "alone and measured, exactly all of it");
        let early = ChainView { hashrate_here: 30_000, hashrate_all: 30_000, network: None, ..ChainView::default() };
        assert_eq!(early.share(), None);
        assert_eq!(ChainView::default().share(), None);
    }
}
