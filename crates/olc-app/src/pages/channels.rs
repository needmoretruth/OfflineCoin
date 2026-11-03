//! Payment channels: coins held with one other computer, paid on at once, and taken back when the
//! channel closes. Every state a channel can be in is said in a sentence.
use super::{Cx, Page, Slipping};
use crate::core::host_channels::{read_request, ChannelView, ChannelsView, PayState, Phase};
use crate::core::{parse_coins, unix_ms, ChannelOrder, Command};
use crate::ui::format;
use crate::ui::tokens;
use crate::ui::widgets::{self, button, button_on, card, field, meta, section, Kind};
use crate::ui::Icon;
use egui::{CollapsingHeader, Ui};
use olc_service::qr::QrGrid;

#[path = "channels_parts.rs"]
mod parts;

const WIDE: f32 = 560.0;
const SHOWN: usize = 5;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[derive(Default)]
pub struct Channels {
    request: String,
    request_fault: Option<String>,
    /// The request being read for whom it pays and the fee, and the number of the answer seen
    /// before it was sent, so an older answer is not taken for this one.
    quoting: Option<(String, u64)>,
    ask_amount: String,
    ask_note: String,
    /// What the other side's channel address field holds.
    open_with: String,
    open_amount: String,
    open_push: String,
    open_delay: String,
    open_fault: Option<String>,
    /// The code of the newest request, made once.
    code: Option<(String, Option<QrGrid>)>,
    /// The code of this computer's channel address, made once and shown on a press.
    address_code: Option<(String, Option<QrGrid>)>,
    show_address_code: bool,
    copied_at: Option<f64>,
}

/// The name of the other computer: its own, or the start of its key.
fn name_of(cx: &Cx, name: &str, key: &str) -> String {
    match name.is_empty() {
        true => cx.fill("ch.unnamed", &[&key[..key.len().min(8)]]),
        false => name.to_string(),
    }
}

/// How a channel stands, as a sentence.
fn state_line(cx: &Cx, channel: &ChannelView) -> String {
    let key = match channel.phase {
        Phase::Asked => "ch.state_asked",
        Phase::Accepted | Phase::FundingSent => "ch.state_agreeing",
        Phase::Confirming => "ch.state_confirming",
        Phase::Open => "ch.state_open",
        Phase::Closing => "ch.state_closing",
        Phase::ForceClosing if channel.cheated => "ch.state_cheated",
        Phase::ForceClosing => match (channel.forced_by_us, channel.blocks_left) {
            (Some(true), Some(blocks)) if blocks > 0 => {
                return cx.fill("ch.state_alone_wait", &[&blocks.to_string()]);
            }
            (Some(false), _) => "ch.state_closed_by_them",
            _ => "ch.state_coming_back",
        },
        Phase::Closed if channel.cheated => "ch.state_cheated_done",
        Phase::Closed => "ch.state_closed",
        Phase::Failed => "ch.state_failed",
    };
    cx.say(key).to_string()
}

impl Channels {
    fn summary(ui: &mut Ui, cx: &Cx, view: &ChannelsView) {
        let open = |channel: &&ChannelView| channel.phase == Phase::Open;
        let ours: u64 = view.channels.iter().filter(open).map(|channel| channel.ours).sum();
        // A channel being closed still has coins on the way to the wallet; they are not gone.
        let coming: u64 = view.channels.iter().map(|channel| channel.coming).sum();
        card(ui, cx.t, |ui| {
            // With nothing open, the coins on their way back are the number, not a 0 above them.
            if ours == 0 && coming > 0 {
                meta(ui, cx.say("ch.coming_mine"), cx.t);
                ui.label(widgets::strong_text(format::coins(coming, cx.ticker())).size(tokens::SLIP_AMOUNT));
                return;
            }
            meta(ui, cx.say("ch.in_channels"), cx.t);
            ui.label(widgets::strong_text(format::coins(ours, cx.ticker())).size(tokens::SLIP_AMOUNT));
            if coming > 0 {
                ui.label(widgets::strong_text(cx.fill("ch.coming", &[&format::coins(coming, cx.ticker())])));
            }
        });
    }

    fn channel_rows(ui: &mut Ui, cx: &mut Cx, view: &ChannelsView) {
        let t = cx.t;
        section(ui, cx.say("ch.channels"));
        ui.add_space(4.0);
        let ended = |channel: &&ChannelView| matches!(channel.phase, Phase::Closed | Phase::Failed);
        let (done, mut rows): (Vec<&ChannelView>, Vec<&ChannelView>) = view.channels.iter().partition(ended);
        if rows.is_empty() {
            meta(ui, cx.say("ch.none"), t);
        }
        // Open ones first, then the rest.
        rows.sort_by_key(|channel| !matches!(channel.phase, Phase::Open | Phase::Closing | Phase::Confirming));
        for channel in rows {
            let peer = hex(&channel.peer.0);
            let name = name_of(cx, &channel.peer_name, &peer);
            let mut order = None;
            card(ui, t, |ui| {
                ui.label(widgets::strong_text(name.clone()));
                meta(ui, &state_line(cx, channel), t);
                ui.add_space(4.0);
                match channel.phase {
                    Phase::Open => Self::room(ui, cx, channel),
                    _ => {
                        ui.horizontal(|ui| {
                            ui.label(cx.fill("ch.yours", &[&format::coins(channel.ours, cx.ticker())]));
                            ui.add_space(tokens::GAP_WIDE);
                            ui.label(cx.fill("ch.theirs", &[&format::coins(channel.theirs, cx.ticker())]));
                        });
                    }
                }
                if matches!(channel.phase, Phase::Open | Phase::Closing) {
                    ui.add_space(tokens::GAP);
                    ui.horizontal(|ui| {
                        if channel.phase == Phase::Open && button(ui, t, cx.say("ch.close"), Kind::Plain).clicked() {
                            order = Some(false);
                        }
                        if button(ui, t, cx.say("ch.close_alone"), Kind::Quiet).clicked() {
                            order = Some(true);
                        }
                    });
                }
            });
            ui.add_space(tokens::GAP);
            if let Some(force) = order {
                let (warning, sign, what) = match force {
                    true => ("ch.alone_warning", "ch.close_alone", "ch.alone_what"),
                    false => ("ch.close_warning", "ch.close", "ch.close_what"),
                };
                let mut slip = cx.slip_for(Slipping {
                    what: cx.say(what),
                    rows: vec![
                        (cx.say("ch.other"), name),
                        (cx.say("ch.mine"), format::coins(channel.ours, cx.ticker())),
                    ],
                    warning: cx.say(warning),
                    sign: cx.say(sign),
                    danger: force,
                    order: vec![Command::Channel(ChannelOrder::Close { channel: hex(&channel.id), force })],
                });
                if force {
                    slip.warning = Some(cx.fill(warning, &[&channel.delay.to_string()]));
                }
                *cx.slip = Some(slip);
            }
        }
        Self::closed_rows(ui, cx, &done);
    }

    /// Channels that are over, folded away: who with, and what came back on the chain.
    fn closed_rows(ui: &mut Ui, cx: &Cx, done: &[&ChannelView]) {
        if done.is_empty() {
            return;
        }
        let t = cx.t;
        ui.add_space(tokens::GAP);
        let header = CollapsingHeader::new(widgets::strong_text(cx.say("ch.closed_list")));
        let now = unix_ms();
        header.id_salt("closed-channels").default_open(false).show(ui, |ui| {
            for channel in done {
                ui.label(widgets::strong_text(name_of(cx, &channel.peer_name, &hex(&channel.peer.0))));
                // When and at which block, so two closed channels with one computer read apart.
                let when = channel.ended_ms.map(|then| format::ago(cx.voice(), now, then));
                let block = channel.closed_at.map(|height| format::number(height.into()));
                let line = match (when, block) {
                    (Some(when), Some(block)) => Some(cx.fill("ch.ended_block", &[&when, &block])),
                    (Some(when), None) => Some(when),
                    (None, Some(block)) => Some(cx.fill("ch.closed_block", &[&block])),
                    (None, None) => None,
                };
                if let Some(line) = line {
                    meta(ui, &line, t);
                }
                meta(ui, &state_line(cx, channel), t);
                if channel.phase == Phase::Closed {
                    ui.label(cx.fill("ch.back", &[&format::coins(channel.back, cx.ticker())]));
                }
                ui.add_space(tokens::GAP);
            }
        });
    }

    fn pay(&mut self, ui: &mut Ui, cx: &mut Cx, view: &ChannelsView) {
        let t = cx.t;
        section(ui, cx.say("ch.pay"));
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if field(ui, &mut self.request, cx.say("ch.pay_hint"), WIDE - 100.0).changed() {
                self.request_fault = None;
                self.quoting = None;
            }
            let ready = !self.request.trim().is_empty() && self.quoting.is_none();
            if button_on(ui, t, cx.say("ch.pay_go"), Kind::Main, ready).clicked() && ready {
                // Read here first, so a wrong paste is said at once; the node then finds the payee
                // and the route's fee for the slip.
                match read_request(&self.request, &cx.snap.node.genesis) {
                    Ok(_) => {
                        let text = self.request.trim().to_string();
                        let seen = view.quote.as_ref().map_or(0, |(number, _, _)| *number);
                        cx.send(Command::Channel(ChannelOrder::Quote { text: text.clone() }));
                        self.quoting = Some((text, seen));
                    }
                    Err(key) => self.request_fault = Some(cx.say(key).to_string()),
                }
            }
        });
        self.quoted(cx, view);
        if let Some(fault) = &self.request_fault {
            meta(ui, fault, t);
        }
    }

    /// The node said whom the request pays and what the route costs: the slip shows both before
    /// anything moves, as a wallet does.
    fn quoted(&mut self, cx: &mut Cx, view: &ChannelsView) {
        let Some((text, seen)) = self.quoting.clone() else {
            return;
        };
        let Some((number, asked, said)) = view.quote.as_ref() else {
            return;
        };
        if *number <= seen || *asked != text {
            return;
        }
        self.quoting = None;
        let quote = match said {
            Ok(quote) => quote,
            Err((key, detail)) => {
                self.request_fault = Some(cx.fill(key, &[detail]));
                return;
            }
        };
        let mut rows = vec![
            (cx.say("ch.to"), name_of(cx, &quote.name, &hex(&quote.payee.0))),
            (cx.say("ch.amount"), format::coins(quote.amount, cx.ticker())),
            (cx.say("ch.fee"), format::coins(quote.fee, cx.ticker())),
        ];
        if !quote.note.is_empty() {
            rows.push((cx.say("ch.note"), quote.note.clone()));
        }
        *cx.slip = Some(cx.slip_for(Slipping {
            what: cx.say("ch.pay"),
            rows,
            warning: cx.say("ch.pay_warning"),
            sign: cx.say("ch.pay_go"),
            danger: false,
            order: vec![Command::Channel(ChannelOrder::Pay { text })],
        }));
        self.request.clear();
    }

    fn ask(&mut self, ui: &mut Ui, cx: &mut Cx, view: &ChannelsView) {
        let t = cx.t;
        section(ui, cx.say("ch.ask"));
        ui.add_space(4.0);
        let amount = parse_coins(self.ask_amount.trim()).filter(|units| *units > 0);
        ui.horizontal(|ui| {
            field(ui, &mut self.ask_amount, cx.say("ch.amount"), 160.0);
            field(ui, &mut self.ask_note, cx.say("ch.ask_note"), WIDE - 280.0);
            if button_on(ui, t, cx.say("ch.ask_go"), Kind::Plain, amount.is_some()).clicked() && amount.is_some() {
                cx.send(Command::Channel(ChannelOrder::Invoice {
                    amount: self.ask_amount.trim().to_string(),
                    note: self.ask_note.trim().to_string(),
                }));
                self.ask_amount.clear();
                self.ask_note.clear();
            }
        });
        self.waiting_code(ui, cx, view);
        self.requests(ui, cx, view);
    }

    fn lately(ui: &mut Ui, cx: &Cx, view: &ChannelsView) {
        let t = cx.t;
        if !view.payments.is_empty() {
            section(ui, cx.say("ch.sent"));
            ui.add_space(4.0);
            for payment in view.payments.iter().take(SHOWN) {
                let state = match payment.state {
                    PayState::Pending => "ch.sent_moving",
                    PayState::Paid => "ch.sent_paid",
                    PayState::Failed => "ch.sent_failed",
                };
                ui.horizontal(|ui| {
                    ui.label(format::coins(payment.amount, cx.ticker()));
                    ui.add_space(tokens::GAP_WIDE);
                    meta(ui, cx.say(state), t);
                });
            }
            ui.add_space(tokens::GAP_WIDE);
        }
        if !view.events.is_empty() {
            section(ui, cx.say("ch.lately"));
            ui.add_space(4.0);
            for event in view.events.iter().take(SHOWN) {
                meta(ui, &cx.fill(event.key, &[&event.detail]), t);
            }
        }
    }
}

impl Page for Channels {
    fn title(&self) -> &'static str {
        "page.channels"
    }

    fn icon(&self) -> Icon {
        Icon::Channel
    }

    fn show(&mut self, ui: &mut Ui, cx: &mut Cx) {
        widgets::title(ui, cx.say("page.channels"));
        let view = cx.snap.node.channels.clone();
        meta(ui, cx.say("ch.about"), cx.t);
        ui.add_space(tokens::GAP_WIDE);
        if !view.running {
            meta(ui, cx.say("ch.no_running"), cx.t);
            return;
        }
        ui.set_max_width(WIDE + 2.0 * tokens::GAP_WIDE);
        // With no channel yet, what to do comes first: the address to hand over and the field
        // for the other one. The balance and the list come once there is something in them.
        self.my_address(ui, cx, &view);
        ui.add_space(tokens::GAP_PAGE);
        if !view.channels.is_empty() {
            Self::summary(ui, cx, &view);
            ui.add_space(tokens::GAP_PAGE);
            Self::channel_rows(ui, cx, &view);
            ui.add_space(tokens::GAP_PAGE);
        }
        self.open(ui, cx, &view);
        ui.add_space(tokens::GAP_PAGE);
        self.pay(ui, cx, &view);
        ui.add_space(tokens::GAP_PAGE);
        self.ask(ui, cx, &view);
        ui.add_space(tokens::GAP_PAGE);
        Self::lately(ui, cx, &view);
    }
}
