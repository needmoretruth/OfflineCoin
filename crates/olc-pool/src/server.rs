//! The pool's port: a thread takes connections, and each connection has a thread that reads its
//! lines and writes what the pool answers and announces. Lines are at most a few kilobytes, a
//! connection that says nothing for long is closed, and the pool holds a fixed number at once.
use crate::pool::Pool;
use crate::session::Reply;
use std::io::{ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Stratum's usual port: miners and pools have offered 3333 since the first Stratum pools.
pub const PORT: u16 = 3333;
/// A line longer than this is no Stratum request.
const MOST_LINE: usize = 4_096;
const TURN: Duration = Duration::from_millis(100);
/// A connection that has not subscribed by then is closed.
const SUBSCRIBE_WITHIN: Duration = Duration::from_secs(30);
/// A connection quiet this long is gone.
const QUIET_FOR: Duration = Duration::from_secs(600);
const WRITE_WITHIN: Duration = Duration::from_secs(5);

pub type Shared = Arc<Mutex<Pool>>;

pub fn unix_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_millis() as u64)
}

pub struct Server {
    stop: Arc<AtomicBool>,
    address: SocketAddr,
    done: Option<JoinHandle<()>>,
}

impl Server {
    /// Listens on `bind`: `127.0.0.1` for this computer alone, `0.0.0.0` for every computer that
    /// can reach it. Port 0 takes any free port.
    pub fn start(bind: SocketAddr, pool: Shared) -> std::io::Result<Self> {
        let listener = TcpListener::bind(bind)?;
        listener.set_nonblocking(true)?;
        let address = listener.local_addr()?;
        let stop = Arc::new(AtomicBool::new(false));
        let watch = stop.clone();
        let done = thread::spawn(move || accept(&listener, &pool, &watch));
        Ok(Self { stop, address, done: Some(done) })
    }

    pub fn address(&self) -> SocketAddr {
        self.address
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(done) = self.done.take() {
            let _ = done.join();
        }
    }
}

fn accept(listener: &TcpListener, pool: &Shared, stop: &Arc<AtomicBool>) {
    let mut connections: Vec<JoinHandle<()>> = Vec::new();
    while !stop.load(Ordering::Relaxed) {
        connections.retain(|connection| !connection.is_finished());
        match listener.accept() {
            Ok((stream, _)) => {
                let (pool, stop) = (pool.clone(), stop.clone());
                connections.push(thread::spawn(move || serve(stream, &pool, &stop)));
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => thread::sleep(TURN / 2),
            Err(_) => thread::sleep(TURN),
        }
    }
    for connection in connections {
        let _ = connection.join();
    }
}

fn serve(mut stream: TcpStream, pool: &Shared, stop: &AtomicBool) {
    let opened = pool.lock().ok().and_then(|mut pool| pool.open(unix_ms()));
    let Some(id) = opened else {
        return;
    };
    let _ = run(&mut stream, pool, stop, id);
    if let Ok(mut pool) = pool.lock() {
        pool.close(id);
    }
}

fn run(stream: &mut TcpStream, pool: &Shared, stop: &AtomicBool, id: u64) -> std::io::Result<()> {
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(TURN))?;
    stream.set_write_timeout(Some(WRITE_WITHIN))?;
    let _ = stream.set_nodelay(true);
    let started = Instant::now();
    let mut heard = Instant::now();
    let mut buffer: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 1_024];
    while !stop.load(Ordering::Relaxed) {
        match stream.read(&mut chunk) {
            Ok(0) => return Ok(()),
            Ok(count) => {
                heard = Instant::now();
                buffer.extend_from_slice(&chunk[..count]);
            }
            Err(error) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
            Err(error) => return Err(error),
        }
        while let Some(end) = buffer.iter().position(|byte| *byte == b'\n') {
            let line: Vec<u8> = buffer.drain(..=end).collect();
            let text = String::from_utf8_lossy(&line);
            if text.trim().is_empty() {
                continue;
            }
            let reply = pool.lock().map(|mut pool| pool.handle(id, &text, unix_ms()));
            let Ok(Reply { lines, keep }) = reply else {
                return Ok(());
            };
            write_all(stream, &lines)?;
            if !keep {
                return Ok(());
            }
        }
        if buffer.len() > MOST_LINE {
            return Ok(());
        }
        let (subscribed, pushes) = match pool.lock() {
            Ok(mut pool) => (pool.is_subscribed(id), pool.pushes(id, unix_ms())),
            Err(_) => return Ok(()),
        };
        write_all(stream, &pushes)?;
        if (!subscribed && started.elapsed() > SUBSCRIBE_WITHIN) || heard.elapsed() > QUIET_FOR {
            return Ok(());
        }
    }
    Ok(())
}

fn write_all(stream: &mut TcpStream, lines: &[String]) -> std::io::Result<()> {
    for line in lines {
        stream.write_all(line.as_bytes())?;
    }
    Ok(())
}

impl Pool {
    pub fn is_subscribed(&self, id: u64) -> bool {
        self.sessions.get(&id).is_some_and(|session| session.subscribed)
    }
}
