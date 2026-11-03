//! Places that hold pages made elsewhere: the blocks, payments (channels and swaps) and the Lab.
//! Each keeps its own page and gets its place, name and icon in the side bar here.
use super::channels::Channels;
use super::lab::Lab;
use super::records::Records;
use super::swap::Swap;
use super::{Cx, Page};
use crate::ui::tokens;
use crate::ui::widgets::{self, segments};
use crate::ui::Icon;
use egui::Ui;

/// The blocks and what is in them: the explorer.
#[derive(Default)]
pub struct Blocks {
    records: Records,
}

impl Page for Blocks {
    fn title(&self) -> &'static str {
        "page.records"
    }

    fn icon(&self) -> Icon {
        Icon::Blocks
    }

    fn show(&mut self, ui: &mut Ui, cx: &mut Cx) {
        self.records.show(ui, cx);
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum PaymentsTab {
    #[default]
    Channels,
    Swap,
}

/// Paying fast through a channel, and trading one coin for another.
#[derive(Default)]
pub struct Payments {
    tab: PaymentsTab,
    channels: Channels,
    swap: Swap,
}

impl Page for Payments {
    fn title(&self) -> &'static str {
        "page.payments"
    }

    fn icon(&self) -> Icon {
        Icon::Channel
    }

    fn show(&mut self, ui: &mut Ui, cx: &mut Cx) {
        let tabs = [
            (PaymentsTab::Channels, cx.say("page.channels").to_string()),
            (PaymentsTab::Swap, cx.say("page.swap").to_string()),
        ];
        if let Some(tab) = segments(ui, cx.t, &tabs, Some(self.tab)) {
            self.tab = tab;
        }
        ui.add_space(tokens::GAP_WIDE);
        match self.tab {
            PaymentsTab::Channels => self.channels.show(ui, cx),
            PaymentsTab::Swap => self.swap.show(ui, cx),
        }
    }
}

/// The 51% attack and the network split, on miners of this computer.
#[derive(Default)]
pub struct LabPage {
    lab: Lab,
}

impl Page for LabPage {
    fn title(&self) -> &'static str {
        "page.lab"
    }

    fn icon(&self) -> Icon {
        Icon::Experiments
    }

    fn show(&mut self, ui: &mut Ui, cx: &mut Cx) {
        widgets::title(ui, cx.say("page.lab"));
        self.lab.show(ui, cx);
    }
}
