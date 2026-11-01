//! The JSON behind the boards on the app's page. Everything here acts with the control key, so it
//! answers only to pages of this app.
use crate::app::{push_line, App};
use crate::web::json_response;
use crate::EXTRAS;
use olc_board::install::app_image;
use olc_board::remote::{command, hash_of, send_image};
use olc_board::{Args, BoardLink, Connection, Extra, RemoteBoard, Say, Seen};
use olc_control::{allowed_remotely, BoardId};
use olc_core::coin::COIN;
use olc_wallet::format_address;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use tiny_http::{Method, Request, Response};

type Reply = Response<std::io::Cursor<Vec<u8>>>;

const RETURN_WAIT: Duration = Duration::from_secs(120);

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

fn board_json(app: &App, seen: &Seen, names: &[Seen]) -> Value {
    let beacon = &seen.beacon;
    let (kind, through) = match &seen.connection {
        Connection::Direct { link } => match app.port_of_link(*link) {
            Some(_) => ("usb", None),
            None => ("network", None),
        },
        Connection::Through { board } => ("radio", *board),
    };
    let through = through.and_then(|id| names.iter().find(|seen| seen.id == id));
    let id = hex(&seen.id);
    json!({
        "id": id,
        "name": beacon.name,
        "kind": kind,
        "through": through.map(|seen| seen.beacon.name.clone()),
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
        "job": app.job(&id),
    })
}

fn boards(app: &App) -> Value {
    let seen = app
        .fleet
        .lock()
        .map(|fleet| fleet.boards())
        .unwrap_or_default();
    let list: Vec<Value> = seen
        .iter()
        .map(|board| board_json(app, board, &seen))
        .collect();
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
    if !allowed_remotely(line) {
        return json!({ "error": "bad_request" });
    }
    match command(&app.fleet, board, line) {
        Ok(answer)
            if answer
                .as_deref()
                .is_some_and(|text| text.starts_with("error")) =>
        {
            json!({ "error": "not_done" })
        }
        Ok(_) => json!({ "ok": true }),
        Err(error) if error.starts_with("refused") => json!({ "error": "refused_order" }),
        Err(_) => json!({ "error": "no_answer" }),
    }
}

/// Sends the build this app holds, then waits for the board to come back with it.
fn update_board(app: &App, board: BoardId, key: &str) -> Result<(), String> {
    app.set_job(key, Some("preparing".into()));
    let image = app_image(&app.firmware)?;
    let total = image.len() as f64;
    let shown = |next: u32| {
        app.set_job(
            key,
            Some(format!("{:.0}%", f64::from(next) * 100.0 / total)),
        )
    };
    send_image(&app.fleet, board, &image, hash_of(&image), &shown)?;
    app.set_job(key, Some("restarting".into()));
    let deadline = Instant::now() + RETURN_WAIT;
    thread::sleep(Duration::from_secs(5));
    while Instant::now() < deadline {
        let back = app.fleet.lock().map(|fleet| {
            fleet
                .boards()
                .iter()
                .any(|seen| seen.id == board && seen.silent_for < Duration::from_secs(3))
        });
        if back.unwrap_or(false) {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(500));
    }
    Err("The board did not come back.".to_string())
}

fn start_update(app: &Arc<App>, request: &Value) -> Value {
    let Some(board) = board_of(request, "id") else {
        return json!({ "error": "bad_request" });
    };
    let app = app.clone();
    thread::spawn(move || {
        let key = hex(&board);
        let result = update_board(&app, board, &key);
        if let Err(error) = &result {
            eprintln!("update: {error}");
        }
        app.set_job(&key, result.err().map(|_| "update_failed".to_string()));
    });
    json!({ "ok": true })
}

fn start_install(app: &Arc<App>, request: &Value) -> Value {
    let (Some(path), Some(name)) = (request["path"].as_str(), request["name"].as_str()) else {
        return json!({ "error": "bad_request" });
    };
    let waiting = app.new_boards().iter().any(|known| known == path);
    if name.trim().is_empty() || !waiting || app.job(path).as_deref() == Some("installing") {
        return json!({ "error": "bad_request" });
    }
    let (app, path, name) = (app.clone(), path.to_string(), name.trim().to_string());
    thread::spawn(move || app.install_on(&path, &name));
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
    let args = Args::parse(words);
    let stop = Arc::new(AtomicBool::new(false));
    {
        let Ok(mut run) = app.run.lock() else {
            return json!({ "error": "busy" });
        };
        if run.running {
            return json!({ "error": "busy" });
        }
        run.lines.clear();
        run.running = true;
        run.stop = Some(stop.clone());
    }
    let links: Vec<Box<dyn BoardLink>> = boards
        .iter()
        .map(|id| Box::new(RemoteBoard::new(app.fleet.clone(), *id)) as Box<dyn BoardLink>)
        .collect();
    let (run, extra): (_, &'static dyn Extra) = (app.run.clone(), *extra);
    let lines = run.clone();
    let say: Say = Arc::new(move |line| push_line(&lines, line));
    thread::spawn(move || {
        let result = extra.run_on(links, &args, say, stop);
        if let Err(error) = result {
            push_line(&run, error);
        }
        if let Ok(mut run) = run.lock() {
            run.running = false;
            run.stop = None;
        }
    });
    json!({ "ok": true })
}

fn stop_extra(app: &App) -> Value {
    if let Ok(run) = app.run.lock() {
        if let Some(stop) = &run.stop {
            stop.store(true, Ordering::Relaxed);
        }
    }
    json!({ "ok": true })
}
