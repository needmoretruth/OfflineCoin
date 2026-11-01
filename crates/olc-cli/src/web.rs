//! The browser screen and the small JSON interface behind it.
use crate::node::Shared;
use crate::{fleet_api, logo};
use olc_core::coin::COIN;
use olc_node::Event;
use olc_wallet::{format_address, parse_address};
use serde_json::{json, Value};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::thread;
use tiny_http::{Header, Method, Request, Response, Server};

const PAGE: &str = include_str!("page.html");
const BOARDS_SCRIPT: &str = include_str!("boards.js");
const RECENT_BLOCKS: u64 = 12;

/// Returns the port it took.
pub fn serve(shared: Arc<Shared>, address: String) -> Result<u16, String> {
    let server = Server::http(&address).map_err(|error| format!("{address}: {error}"))?;
    let port = server
        .server_addr()
        .to_ip()
        .map_or(0, |address| address.port());
    thread::spawn(move || {
        for request in server.incoming_requests() {
            // A command waits for its board, which must not hold up the page.
            let shared = shared.clone();
            thread::spawn(move || handle(&shared, request));
        }
    });
    Ok(port)
}

pub fn header(name: &str, value: &str) -> Option<Header> {
    Header::from_bytes(name.as_bytes(), value.as_bytes()).ok()
}

fn handle(shared: &Shared, mut request: Request) {
    let path = request.url().split('?').next().unwrap_or("/").to_string();
    let method = request.method().clone();
    let mut body = String::new();
    let _ = request.as_reader().read_to_string(&mut body);
    if let Some(app) = &shared.app {
        if let Some(response) = fleet_api::answer(app, &request, &method, &path, &body) {
            let _ = request.respond(response);
            return;
        }
    }
    let response = match (method, path.as_str()) {
        (Method::Get, "/") => {
            let mut page = Response::from_string(PAGE);
            if let Some(kind) = header("Content-Type", "text/html; charset=utf-8") {
                page.add_header(kind);
            }
            page
        }
        (Method::Get, "/boards.js") => Response::from_string(BOARDS_SCRIPT),
        (Method::Get, "/api/status") => json_response(&status(shared)),
        (Method::Get, "/api/logo") => json_response(&json!(logo::frames_for_page())),
        (Method::Post, "/api/send") => json_response(&send(shared, &body)),
        (Method::Post, "/api/mining") => json_response(&set_mining(shared, &body)),
        _ => Response::from_string("not found").with_status_code(404),
    };
    let _ = request.respond(response);
}

pub fn json_response(value: &Value) -> Response<std::io::Cursor<Vec<u8>>> {
    let mut response = Response::from_string(value.to_string());
    if let Some(kind) = header("Content-Type", "application/json") {
        response.add_header(kind);
    }
    response
}

fn coins(units: u64) -> String {
    format!("{}.{:02}", units / COIN, units % COIN / (COIN / 100))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn event_json(time_ms: u64, event: &Event) -> Value {
    let (kind, height, detail) = match event {
        Event::Mined { height } => ("mined", *height, String::new()),
        Event::BlockAccepted { height } => ("accepted", *height, String::new()),
        Event::Reorganized { replaced, height } => ("reorg", *height, replaced.to_string()),
        Event::BlockRejected { reason } => ("block_rejected", 0, format!("{reason:?}")),
        Event::PaymentReceived { amount } => ("money_in", 0, coins(*amount)),
        Event::PaymentSent { amount } => ("money_out", 0, coins(*amount)),
        Event::TxAccepted => ("tx", 0, String::new()),
        Event::TxRejected { reason } => ("tx_rejected", 0, format!("{reason:?}")),
        Event::TxReturned { id } => ("tx_returned", 0, hex(id)),
        Event::TxDropped { id, .. } => ("tx_dropped", 0, hex(id)),
    };
    json!({ "time": time_ms, "kind": kind, "height": height, "detail": detail })
}

fn status(shared: &Shared) -> Value {
    let Ok(runner) = shared.runner.lock() else {
        return json!({});
    };
    let engine = &runner.engine;
    let chain = engine.chain();
    let status = engine.status();
    let first = status.height.saturating_sub(RECENT_BLOCKS - 1);
    let blocks: Vec<Value> = chain
        .best_chain_from(first)
        .iter()
        .enumerate()
        .map(|(offset, block)| {
            let miner = block.txs[0]
                .outputs
                .first()
                .map(|out| hex(&out.lock))
                .unwrap_or_default();
            json!({
                "height": first + offset as u64,
                "id": hex(&block.id()),
                "time": block.header.time_ms,
                "txs": block.txs.len() - 1,
                "miner": miner,
            })
        })
        .collect();
    let events: Vec<Value> = shared
        .log
        .lock()
        .map(|log| {
            log.iter()
                .map(|(time, event)| event_json(*time, event))
                .collect()
        })
        .unwrap_or_default();
    let config = chain.config();
    json!({
        "name": config.name,
        "ticker": config.ticker,
        "consensus": chain.plugins().consensus.name(),
        "block_time_ms": config.rules.block_time_ms,
        "height": status.height,
        "tip": hex(&status.tip),
        "difficulty": status.difficulty,
        "balance": status.balance as f64 / COIN as f64,
        "address": format_address(&engine.address()),
        "self": hex(&engine.address()),
        "pool": status.pool,
        "peers": status.peers,
        "mining": shared.mining.load(Ordering::Relaxed),
        "hashrate": shared.hashrate.load(Ordering::Relaxed),
        "siblings": shared.siblings,
        "blocks": blocks,
        "events": events,
    })
}

fn send(shared: &Shared, body: &str) -> Value {
    let request: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    let to = request["to"].as_str().and_then(parse_address);
    let amount = request["amount"].as_f64().filter(|amount| *amount > 0.0);
    let fee = request["fee"].as_f64().unwrap_or(0.0);
    let (Some(to), Some(amount)) = (to, amount) else {
        return json!({ "error": "bad_request" });
    };
    let Ok(mut runner) = shared.runner.lock() else {
        return json!({ "error": "busy" });
    };
    let units = |coins: f64| (coins * COIN as f64).round() as u64;
    match runner.engine.pay(to, units(amount), units(fee)) {
        Ok(id) => json!({ "id": hex(&id) }),
        Err(error) => json!({ "error": format!("{error:?}") }),
    }
}

fn set_mining(shared: &Shared, body: &str) -> Value {
    let request: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    let on = request["on"].as_bool().unwrap_or(false);
    shared.mining.store(on, Ordering::Relaxed);
    json!({ "mining": on })
}
