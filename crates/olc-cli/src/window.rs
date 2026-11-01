//! Opens the app for someone who never typed a flag: one running copy, in a window of its own
//! when a Chromium browser is installed.
use crate::app::App;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{self, Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

const CHROMIUM_BROWSERS: [&str; 7] = [
    "brave-browser",
    "brave",
    "chromium-browser",
    "chromium",
    "google-chrome",
    "google-chrome-stable",
    "microsoft-edge",
];
const PROBE_WAIT: Duration = Duration::from_secs(2);
/// A browser tab that closes says nothing, so a page unheard this long is gone. Background tabs
/// keep polling at least once a minute.
const PAGE_GONE: Duration = Duration::from_secs(90);
const WATCH_STEP: Duration = Duration::from_secs(1);

fn port_file(folder: &Path) -> PathBuf {
    folder.join("app.port")
}

fn has_a_display() -> bool {
    ["DISPLAY", "WAYLAND_DISPLAY"]
        .iter()
        .any(|name| std::env::var_os(name).is_some_and(|value| !value.is_empty()))
}

fn in_path(program: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|folder| folder.join(program))
        .find(|file| file.is_file())
}

fn spawn_quietly(command: &mut Command) -> Option<Child> {
    let silent = || Stdio::null();
    let started = command
        .stdin(silent())
        .stdout(silent())
        .stderr(silent())
        .spawn();
    started.ok()
}

/// Its own profile keeps it a separate process, so it lives exactly as long as the window, even
/// when the same browser is already open.
fn open_window(url: &str, folder: &Path) -> Option<Child> {
    let browser = CHROMIUM_BROWSERS.iter().find_map(|name| in_path(name))?;
    let profile = folder.join("window");
    spawn_quietly(
        Command::new(browser)
            .arg(format!("--app={url}"))
            .arg(format!("--user-data-dir={}", profile.display()))
            .args(["--no-first-run", "--no-default-browser-check"]),
    )
}

fn open_in_default_browser(url: &str) {
    let _ = spawn_quietly(Command::new("xdg-open").arg(url));
}

/// True when a copy of the app answers on `port`.
fn answers(port: u16) -> bool {
    let Ok(mut stream) = TcpStream::connect(("127.0.0.1", port)) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(PROBE_WAIT));
    let request = format!("GET /api/boards HTTP/1.0\r\nHost: 127.0.0.1:{port}\r\n\r\n");
    let mut reply = String::new();
    let _ = stream.write_all(request.as_bytes());
    let _ = stream.read_to_string(&mut reply);
    reply.starts_with("HTTP/1.1 200") || reply.starts_with("HTTP/1.0 200")
}

/// Shows the app that is already running, if there is one, instead of a second node fighting it
/// for the cables.
pub fn open_existing(folder: &Path) -> bool {
    let saved = fs::read_to_string(port_file(folder)).unwrap_or_default();
    let Ok(port) = saved.trim().parse() else {
        return false;
    };
    if !answers(port) {
        return false;
    }
    let url = format!("http://127.0.0.1:{port}");
    println!("{url}");
    if has_a_display() && open_window(&url, folder).is_none() {
        open_in_default_browser(&url);
    }
    true
}

/// Held for as long as this copy runs. Two copies started together cannot both pass.
pub fn claim(folder: &Path) -> Result<File, String> {
    fs::create_dir_all(folder).map_err(|error| error.to_string())?;
    let lock = File::create(folder.join("app.lock")).map_err(|error| error.to_string())?;
    lock.try_lock()
        .map_err(|_| String::from("The app is already starting."))?;
    Ok(lock)
}

fn leave(folder: &Path, window: &Mutex<Option<Child>>) -> ! {
    let _ = fs::remove_file(port_file(folder));
    if let Ok(mut window) = window.lock() {
        if let Some(window) = window.as_mut() {
            let _ = window.kill();
        }
    }
    process::exit(0);
}

/// Opens the page and ends this process cleanly once it is closed, or when the terminal closes,
/// which also lets go of the cables.
pub fn open_and_watch(port: u16, folder: PathBuf, app: Arc<App>) -> Result<(), String> {
    fs::write(port_file(&folder), format!("{port}")).map_err(|error| error.to_string())?;
    let url = format!("http://127.0.0.1:{port}");
    println!("{url}");
    let opened = has_a_display().then(|| open_window(&url, &folder));
    let in_a_window = matches!(opened, Some(Some(_)));
    if opened.is_some() && !in_a_window {
        open_in_default_browser(&url);
    }
    let window = Arc::new(Mutex::new(opened.flatten()));
    let stopping = (folder.clone(), window.clone());
    ctrlc::set_handler(move || leave(&stopping.0, &stopping.1))
        .map_err(|error| error.to_string())?;
    if has_a_display() {
        thread::spawn(move || loop {
            thread::sleep(WATCH_STEP);
            let closed = match window.lock() {
                Ok(mut child) => match child.as_mut() {
                    Some(child) => !matches!(child.try_wait(), Ok(None)),
                    None => app.page_gone_for(PAGE_GONE),
                },
                Err(_) => true,
            };
            if closed {
                leave(&folder, &window);
            }
        });
    }
    Ok(())
}
