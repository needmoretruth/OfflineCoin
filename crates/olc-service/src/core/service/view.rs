//! The snapshot the window reads: the node, the boards, the computers around and what runs here.
use super::super::chain_race::{complete, other_tips};
use super::super::coins::coin_folder;
use super::super::host_parts::unix_ms;
use super::super::found::link_of;
use super::super::returning::is_returning;
use super::super::rows::{board_entry, coin_of_board, earlier_rules, keeps_of, merge_local, on_other_rules, own_entry, reach_of_board};
use super::super::snapshot::{BoardView, ExperimentView, ScanView, Snapshot};
use super::Service;
use crate::discovery::{group, Identity};
use crate::fleet::BoardRow;
use std::sync::Arc;

impl Service {
    pub(super) fn publish(&mut self) {
        let mut node = self.host.view();
        let identity = node.coin.clone().map(Identity::of);
        let rows = self.fleet.as_ref().map(|fleet| fleet.board_rows());
        let mut rows = rows.unwrap_or_default();
        self.returning.mark(&mut rows);
        let mut entries = self
            .found
            .entries((self.host.address(), node.port.unwrap_or(0)));
        entries.push(own_entry(&node, identity.clone()));
        let said = |row: &BoardRow| {
            let by_link = self.fleet.as_ref().and_then(|fleet| fleet.hello_of(row.seen.id));
            by_link.or_else(|| self.found.board(&row.seen.beacon.address).cloned())
        };
        let hellos: Vec<_> = rows.iter().map(said).collect();
        let links: Vec<_> = hellos.iter().map(|hello| hello.as_ref().and_then(link_of)).collect();
        let answers = hellos.iter().map(|hello| hello.as_ref().map(|hello| Identity::of(hello.coin.clone())));
        let answers: Vec<_> = answers.collect();
        let mut known: Vec<_> = entries.iter().filter_map(|entry| entry.identity.clone()).collect();
        known.extend(answers.iter().flatten().cloned());
        known.extend(node.coin.iter().flat_map(earlier_rules));
        let told = self.told.lock().map(|known| known.clone()).unwrap_or_default();
        let said_rules = |row: &BoardRow| told.get(&row.seen.id).and_then(|told| told.state.rules.clone());
        entries.extend(rows.iter().zip(answers).zip(&hellos).map(|((row, said), hello)| {
            let rules = said_rules(row);
            let coin = coin_of_board((row.seen.beacon.genesis, rules.as_deref()), said, &known);
            let cooling = told.get(&row.seen.id).is_some_and(|told| told.state.cooling == Some(true));
            board_entry(row, coin, hello.as_ref(), cooling)
        }));
        if let Some(fleet) = &self.fleet {
            let auto = self.settings.auto_update;
            let boards = self.settings.auto_boards.iter().filter(|_| auto);
            fleet.keep_updated(boards.copied().collect());
        }
        let specs = self.settings.nodes.clone();
        let local: Vec<_> = specs.iter().map(|spec| self.local.view(spec)).collect();
        merge_local(&mut entries, &local, identity.as_ref());
        let boards: Vec<BoardView> = rows
            .into_iter()
            .zip(links)
            .zip(hellos.iter().map(|hello| keeps_of(hello.as_ref())))
            .map(|((row, link), keeps)| BoardView {
                other_rules: on_other_rules(
                    (row.seen.beacon.genesis, said_rules(&row).as_deref()),
                    node.coin.as_ref(),
                ),
                keeps,
                returning: is_returning(&row),
                reach: reach_of_board(&row, link),
                state: told.get(&row.seen.id).map(|told| told.state.clone()).unwrap_or_default(),
                answer: told.get(&row.seen.id).and_then(|told| told.answer.clone()),
                seen: row.seen,
                job: row.job,
            })
            .collect();
        self.host.watch_tips(other_tips(&node, &local, &boards));
        let mut chain = std::mem::take(&mut node.chain);
        complete(&mut chain, &node, &local, &boards, &mut self.trail, unix_ms());
        node.chain = chain;
        let fleet = self.fleet.as_ref();
        let new_ports = fleet.map_or_else(Vec::new, |fleet| {
            let ports = fleet.new_boards();
            ports
                .into_iter()
                .map(|port| {
                    let job = fleet.job(&port);
                    (port, job)
                })
                .collect()
        });
        let (lines, running) = fleet.map_or_else(Default::default, |fleet| fleet.run_log());
        let snapshot = Snapshot {
            groups: group(&entries, identity.as_ref()),
            swaps: self.swaps.view((&node.coin.as_ref().map(coin_folder).unwrap_or_default(), node.height)),
            node,
            boards,
            local,
            new_ports,
            scan: ScanView {
                running: self.scanner.running(),
                tried: self.scanner.tried(),
                subnets: self.scanner.subnets(),
                age_secs: self.scanner.age().map(|age| age.as_secs()),
            },
            experiment: ExperimentView { lines, running },
            pool: self.pool.view(),
            notice: self.notice.clone(),
            usb: self.usb,
            full_rate: self.full_rate,
            mine_on_start: self.settings.mine_on_start,
            folder: self.paths.root.display().to_string(),
            made_ms: unix_ms(),
            old_olc_note: self.settings.old_olc_note,
        };
        if let Ok(mut out) = self.out.lock() {
            *out = Arc::new(snapshot);
        }
    }
}
