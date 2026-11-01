//! The JSON behind the boards on the app's page. Everything here acts with the control key, so it
//! answers only to pages of this app.
use crate::app::App;
use crate::web::json_response;
use crate::EXTRAS;
use olc_board::manager::{BoardRow, LineError};
use olc_board::{Args, Connection};
use olc_control::BoardId;
use olc_core::coin::COIN;
use olc_wallet::format_address;
use serde_json::{json, Value};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tiny_http::{Method, Request, Response};

type Reply = Response<std::io::Cursor<Vec<u8>>>;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn parse_id(text: &str) -> Option<BoardId> {
    let mut id = [0u8; 6];
    if text.len() != 12 || !text.is_ascii() {
        return None;
    }
    for (index, byte) in id.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16).ok()?;
    }
    Some(id)
}

/// A web page anywhere can post to this computer, so the request must come from the app's own.
fn from_our_page(app: &App, request: &Request) -> bool {
    let value = |name: &'static str| {
        request
            .headers()
            .iter()
            .find(|found| found.field.equiv(name))
            .map(|found| found.value.as_str().to_string())
    };
    let hosts = [
        format!("127.0.0.1:{}", app.web_port.load(Ordering::Relaxed)),
        format!("localhost:{}", app.web_port.load(Ordering::Relaxed)),
    ];
    let host_fits = value("Host").is_some_and(|host| hosts.contains(&host));
    let origin_fits = value("Origin").is_none_or(|origin| {
        origin
            .strip_prefix("http://")
            .is_some_and(|host| hosts.iter().any(|known| known == host))
    });
    host_fits && origin_fits
}

pub fn answer(
    app: &Arc<App>,
    request: &Request,
    method: &Method,
    path: &str,
    body: &str,
) -> Option<Reply> {
    let script = path
        .strip_prefix("/extra/")
        .and_then(|name| name.strip_suffix(".js"));
    if let (Method::Get, Some(name)) = (method, script) {
        let extra = EXTRAS.iter().find(|extra| extra.command() == name)?;
        return Some(Response::from_string(extra.script()));
    }
    let known = matches!(
        path,
        "/api/boards"
            | "/api/board"
            | "/api/update"
            | "/api/install"
            | "/api/extra/start"
            | "/api/extra/stop"
    );
    if !known {
        return None;
    }
    if !from_our_page(app, request) {
        return Some(Response::from_string("not allowed").with_status_code(403));
    }
    let request: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    Some(json_response(&match (method, path) {
        (Method::Get, "/api/boards") => {
            app.page_was_here();
            boards(app)
        }
        (Method::Post, "/api/board") => run_line(app, &request),
        (Method::Post, "/api/update") => start_update(app, &request),
        (Method::Post, "/api/install") => start_install(app, &request),
        (Method::Post, "/api/extra/start") => start_extra(app, &request),
        (Method::Post, "/api/extra/stop") => stop_extra(app),
        _ => json!({ "error": "bad_request" }),
    }))
}

fn board_json(row: &BoardRow) -> Value {
    let seen = &row.seen;
    let beacon = &seen.beacon;
    let kind = match (row.on_cable, &seen.connection) {
        (true, _) => "usb",
        (false, Connection::Direct { .. }) => "network",
        (false, Connection::Through { .. }) => "radio",
    };
    json!({
        "id": hex(&seen.id),
        "name": beacon.name,
        "kind": kind,
        "through": row.through,
        "address": format_address(&beacon.address),
        "height": beacon.height,
        "balance": beacon.balance as f64 / COIN as f64,
        "hashrate": beacon.hashrate,
        "limit": beacon.limit,
        "peers": beacon.peers,
        "pool": beacon.pool,
        "mining": beacon.mining,
        "offline": beacon.offline,
        "view": beacon.view,
        "language": beacon.language,
        "build": beacon.build,
        "trusted": beacon.trusts_a_key,
        "refused": beacon.refused,
        "online": seen.online,
        "silent": seen.silent_for.as_secs(),
        "job": row.job,
    })
}

fn boards(app: &App) -> Value {
    let list: Vec<Value> = app.board_rows().iter().map(board_json).collect();
    let ports: Vec<Value> = app
        .new_boards()
        .into_iter()
        .map(|path| json!({ "path": path, "job": app.job(&path) }))
        .collect();
    let extras: Vec<&str> = EXTRAS.iter().map(|extra| extra.command()).collect();
    let run = app.run.lock();
    let (lines, running) = run
        .map(|run| (run.lines.iter().cloned().collect::<Vec<_>>(), run.running))
        .unwrap_or_default();
    json!({
        "boards": list,
        "ports": ports,
        "extras": extras,
        "run": { "lines": lines, "running": running },
    })
}

fn board_of(request: &Value, key: &str) -> Option<BoardId> {
    request[key].as_str().and_then(parse_id)
}

fn run_line(app: &App, request: &Value) -> Value {
    let (Some(board), Some(line)) = (board_of(request, "id"), request["line"].as_str()) else {
        return json!({ "error": "bad_request" });
    };
    match app.ask(board, line) {
        Ok(answer) => json!({ "ok": true, "answer": answer }),
        Err(LineError::BadRequest) => json!({ "error": "bad_request" }),
        Err(LineError::NotDone) => json!({ "error": "not_done" }),
        Err(LineError::Refused) => json!({ "error": "refused_order" }),
        Err(LineError::NoAnswer) => json!({ "error": "no_answer" }),
    }
}

fn start_update(app: &Arc<App>, request: &Value) -> Value {
    let Some(board) = board_of(request, "id") else {
        return json!({ "error": "bad_request" });
    };
    app.start_update(board);
    json!({ "ok": true })
}

fn start_install(app: &Arc<App>, request: &Value) -> Value {
    let (Some(path), Some(name)) = (request["path"].as_str(), request["name"].as_str()) else {
        return json!({ "error": "bad_request" });
    };
    if !app.start_install(path, name) {
        return json!({ "error": "bad_request" });
    }
    json!({ "ok": true })
}

fn start_extra(app: &Arc<App>, request: &Value) -> Value {
    let extra = EXTRAS
        .iter()
        .find(|extra| Some(extra.command()) == request["command"].as_str());
    let boards: Option<Vec<BoardId>> = request["boards"].as_array().map(|list| {
        list.iter()
            .filter_map(|id| id.as_str().and_then(parse_id))
            .collect()
    });
    let (Some(extra), Some(boards)) = (extra, boards) else {
        return json!({ "error": "bad_request" });
    };
    let words = request["args"].as_array().cloned().unwrap_or_default();
    let words = words
        .iter()
        .filter_map(|word| word.as_str().map(String::from));
    match app.start_extra(*extra, &boards, Args::parse(words)) {
        Ok(()) => json!({ "ok": true }),
        Err(_) => json!({ "error": "busy" }),
    }
}

fn stop_extra(app: &App) -> Value {
    app.stop_extra();
    json!({ "ok": true })
}
