//! The lines a board answers with when asked how it is doing.
use crate::commands::opened_name;
use crate::control::BUILD;
use crate::node::{decode_hash, hex, Node, LINK_RADIO, LINK_USB, LINK_WIFI};
use crate::radio;
use crate::screen::{PAGES, WIDTH};
use crate::views::coins;
use alloc::format;
use core::sync::atomic::Ordering;
use esp_hal::peripherals::GPIO;
use esp_println::println;
use olc_control::bootlog;
use olc_core::Hash;
use olc_wallet::format_address;

impl Node {
    /// Prints what was drawn and what the panel's own memory holds, row by row.
    pub fn print_screen(&mut self) {
        let mut same = self.screen.is_connected();
        println!(
            "screen connected={same} spilled={}",
            self.screen.take_spilled()
        );
        for page in 0..PAGES {
            let mut held = [0u8; WIDTH];
            let read = self.screen.read_row(page, &mut held).is_ok();
            let drawn = self.screen.row(page);
            same &= read && drawn == held;
            println!("row {page} {} {}", hex(drawn), hex(&held));
        }
        println!("screen matches={same}");
    }

    pub fn print_tx(&mut self, text: &str) {
        let mut id: Hash = [0; 32];
        let engine = &self.runner.engine;
        let line = match decode_hash(text, &mut id) {
            false => "error tx ID".into(),
            true => match engine.chain().find_tx(&id) {
                Some(height) => format!("tx {text} height={height}"),
                None if engine.in_pool(&id) => format!("tx {text} pool"),
                None => match self.dropped.iter().find(|(known, _)| *known == id) {
                    Some((_, reason)) => format!("tx {text} dropped={reason}"),
                    None => format!("tx {text} none"),
                },
            },
        };
        self.say(line);
    }

    pub fn temperature(&self) -> f32 {
        crate::thermo::read().unwrap_or(f32::NAN)
    }

    pub fn print_status(&mut self) {
        let status = self.runner.engine.status();
        let chain = self.runner.engine.chain();
        let held = chain.held();
        let line = format!(
            "status height={} tip={} work={} difficulty={} balance={} peers={} espnow={} usb={} \
             wifi={} pool={} hashrate={} limit={} chosen={} mining={} offline={} view={} lang={} \
             screen={} free={} router={:?} address={} reorg={}/{} sync={} build={BUILD} slot={} \
             temp={:.1} reset={} panel_ms={} read_ms={} placed_ms={} loaded_ms={} smooth={} fps={} frame_us={} \
             power={} rssi={} bright={} busy={} clock={} boot_wdt={} strap={:#x} key={} name={} \
             opened={} oldest={} held={} fits={} keep={} unspent={} miner={} merge={} needs={} \
             assumed={} work_on={} near={} dials={} moves={} links={}",
            status.height,
            hex(&status.tip),
            status.work,
            status.difficulty,
            coins(status.balance),
            status.peers,
            self.runner.peers_on(LINK_RADIO),
            self.runner.peers_on(LINK_USB),
            self.runner.peers_on(LINK_WIFI),
            status.pool,
            self.rate,
            self.limit,
            self.limit_chosen,
            self.mining,
            self.runner.offline,
            self.view.name(),
            self.language.name(),
            self.screen.is_connected(),
            esp_alloc::HEAP.free(),
            self.network.router_address(),
            format_address(&self.runner.engine.address()),
            self.last_reorg.0,
            self.last_reorg.1,
            status.syncing_to.unwrap_or(0),
            self.control.slot,
            self.temperature(),
            bootlog::reason_name(self.started.reset),
            self.started.panel_ms,
            self.started.read_ms,
            self.started.placed_ms,
            self.started.loaded_ms,
            self.smooth,
            self.frames.last_second,
            self.frames.last_slowest_us,
            self.power_dbm,
            radio::LAST_RSSI.load(Ordering::Relaxed),
            self.bright,
            self.busy_percent,
            self.clock_mhz,
            self.boot_watchdog,
            GPIO::regs().strap().read().bits(),
            self.control.gate.has_key(),
            self.control.identity.name,
            opened_name(chain.opened()),
            held.oldest,
            held.bytes,
            held.fits.unwrap_or(0),
            held.recent.unwrap_or(0),
            chain.unspent_count(),
            format_address(&self.runner.engine.miner()),
            self.runner.engine.merging(),
            status.needs_history.map_or(0, |(_, height)| height),
            status.assumed_from.map_or(-1, i64::from),
            self.work.is_some(),
            self.runner.neighbour_count(),
            self.runner.dials(),
            self.runner.neighbours().iter().map(|near| near.moves).sum::<u32>(),
            self.runner.rank().text(),
        );
        self.say(line);
    }
}
