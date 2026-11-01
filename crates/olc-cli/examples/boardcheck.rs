//! Proves on real boards what the page alone cannot: that a board refuses orders it should, and
//! how long an update takes. Run with no app holding the port:
//!   boardcheck PORT BOARD_ID refuse [FIRMWARE]
//!   boardcheck PORT BOARD_ID update FIRMWARE
use olc_board::control_key::{config_folder, ControlKey};
use olc_board::install::app_image;
use olc_board::remote::{command, hash_of, send_image};
use olc_board::{Fleet, SharedFleet};
use olc_control::{sign, Action, BoardId, Body, Forward, Order, Signed};
use olc_link_serial::SerialLink;
use olc_node::{network, Link, Message, Runner, Target};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const WHO_WAIT: Duration = Duration::from_secs(70);
const WHO_EVERY: Duration = Duration::from_secs(10);
const LOOK: Duration = Duration::from_millis(2);
const REFUSAL_WAIT: Duration = Duration::from_secs(4);
const BOARD_WAIT: Duration = Duration::from_secs(30);
const RETURN_WAIT: Duration = Duration::from_secs(180);

fn parse_id(text: &str) -> Option<BoardId> {
    let mut id = [0u8; 6];
    for (index, byte) in id.iter_mut().enumerate() {
        *byte = u8::from_str_radix(text.get(index * 2..index * 2 + 2)?, 16).ok()?;
    }
    Some(id)
}

fn pump(fleet: SharedFleet, mut link: SerialLink) {
    thread::spawn(move || loop {
        while let Some((peer, bytes)) = link.receive() {
            let message = network::read(&bytes).and_then(|(_, body)| Message::decode(body));
            if let Some(Message::Control(bytes)) = message {
                if let Ok(mut fleet) = fleet.lock() {
                    fleet.hear(peer, &bytes);
                }
            }
        }
        while let Some(out) = fleet
            .lock()
            .ok()
            .and_then(|mut fleet| fleet.take_outgoing())
        {
            if out.0 != Forward::Nowhere {
                let message = Message::Control(out.1).encode();
                link.send(Target::All, &network::stamp(network::CONTROL, &message));
            }
        }
        thread::sleep(LOOK);
    });
}

fn beacon(fleet: &SharedFleet, board: BoardId) -> Option<olc_control::Beacon> {
    let fleet = fleet.lock().ok()?;
    let seen = fleet.boards().into_iter().find(|seen| seen.id == board)?;
    Some(seen.beacon)
}

fn refused_after(fleet: &SharedFleet, board: BoardId, what: &str) {
    let deadline = Instant::now() + REFUSAL_WAIT;
    while Instant::now() < deadline {
        let reason = fleet
            .lock()
            .ok()
            .and_then(|mut fleet| fleet.take_refusal(board));
        if let Some(reason) = reason {
            println!("{what}: refused, {reason:?}");
            return;
        }
        thread::sleep(LOOK);
    }
    println!("{what}: NO REFUSAL");
}

fn send(fleet: &SharedFleet, board: BoardId, body: Body) {
    if let Ok(mut fleet) = fleet.lock() {
        fleet.send_body(board, body);
    }
}

fn refusals(fleet: &SharedFleet, board: BoardId, firmware: Option<&str>) -> Result<(), String> {
    let limit = beacon(fleet, board).ok_or("no beacon")?.limit;
    let order = |counter, target| Order {
        board: target,
        counter,
        action: Action::Line(format!("limit {limit}")),
    };
    let next = beacon(fleet, board).ok_or("no beacon")?.counter + 1;
    let unsigned = Signed {
        order: order(next, board),
        signature: vec![0; 64],
    };
    send(fleet, board, Body::Order(unsigned));
    refused_after(fleet, board, "unsigned order");

    let secret = [7u8; 32];
    let foreign = sign(&secret, order(next, board)).ok_or("bad key")?;
    send(fleet, board, Body::Order(foreign));
    refused_after(fleet, board, "order signed with another key");

    let key = ControlKey::load_or_create(&config_folder()?)?;
    let good = sign(key.secret(), order(next, board)).ok_or("bad key")?;
    send(fleet, board, Body::Order(good.clone()));
    thread::sleep(Duration::from_secs(2));
    send(fleet, board, Body::Order(good));
    refused_after(fleet, board, "replayed order");

    let mut other = board;
    other[5] ^= 1;
    let wrong = sign(key.secret(), order(next + 1, other)).ok_or("bad key")?;
    send(fleet, board, Body::Order(wrong));
    refused_after(fleet, board, "order for a different board");

    let trust = sign(
        key.secret(),
        Order {
            board,
            counter: next + 1,
            action: Action::Line("trust 00".into()),
        },
    );
    send(fleet, board, Body::Order(trust.ok_or("bad key")?));
    refused_after(fleet, board, "changing the key by radio");

    if let Some(firmware) = firmware {
        let mut image = app_image(firmware.as_ref())?;
        let hash = hash_of(&image);
        let middle = image.len() / 2;
        image[middle] ^= 1;
        let build = beacon(fleet, board).ok_or("no beacon")?.build;
        let result = send_image(fleet, board, &image, hash, &|_| {});
        println!("image with one flipped byte: {result:?}");
        thread::sleep(Duration::from_secs(8));
        let now = beacon(fleet, board).ok_or("no beacon")?;
        println!(
            "still running {} (was {build}), height {}",
            now.build, now.height
        );
    }
    let shown = beacon(fleet, board).ok_or("no beacon")?;
    println!("the board counts {} refusals", shown.refused);
    Ok(())
}

fn update(fleet: &SharedFleet, board: BoardId, firmware: &str) -> Result<(), String> {
    let before = beacon(fleet, board).ok_or("no beacon")?;
    let image = app_image(firmware.as_ref())?;
    println!(
        "{} bytes, build {} height {} slot {}",
        image.len(),
        before.build,
        before.height,
        before.slot
    );
    let mut percent = 0;
    let shown = Mutex::new(&mut percent);
    let total = image.len() as u64;
    let took = send_image(fleet, board, &image, hash_of(&image), &|next| {
        if let Ok(mut percent) = shown.lock() {
            let now = u64::from(next) * 10 / total;
            if now > **percent {
                **percent = now;
                println!("{}0%", now);
            }
        }
    })?;
    println!("sent in {:.1} s", took.as_secs_f64());
    let deadline = Instant::now() + RETURN_WAIT;
    while Instant::now() < deadline {
        let now = beacon(fleet, board).ok_or("no beacon")?;
        if now.slot != before.slot {
            println!(
                "back on build {} slot {} height {}, {:.1} s after the last byte",
                now.build,
                now.slot,
                now.height,
                (RETURN_WAIT - (deadline - Instant::now())).as_secs_f64()
            );
            return Ok(());
        }
        thread::sleep(Duration::from_millis(100));
    }
    Err("the board did not come back".into())
}

/// Prints what the boards around say they are, heard through the one on the cable.
fn who(mut link: SerialLink) {
    let start = Instant::now();
    let mut asked = start - WHO_EVERY;
    while start.elapsed() < WHO_WAIT {
        if asked.elapsed() >= WHO_EVERY {
            asked = Instant::now();
            let message = Message::Who.encode();
            link.send(Target::All, &network::stamp(network::OPEN, &message));
        }
        while let Some((_, bytes)) = link.receive() {
            let message = network::read(&bytes).and_then(|(_, body)| Message::decode(body));
            if let Some(Message::Hello(hello)) = message {
                let genesis = hello.genesis();
                println!(
                    "{:.1} s: {} {} {} coin {:02x}{:02x} height {} work {} address {:02x}{:02x}",
                    start.elapsed().as_secs_f64(),
                    hello.card.name,
                    hello.card.device,
                    hello.card.build,
                    genesis[0],
                    genesis[1],
                    hello.height,
                    hello.work,
                    hello.address[0],
                    hello.address[1],
                );
            }
        }
        thread::sleep(LOOK);
    }
}

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [port, board, what, rest @ ..] = args.as_slice() else {
        return Err("boardcheck PORT BOARD_ID refuse|update [FIRMWARE]".into());
    };
    if what == "loader" {
        println!("{:x?}", olc_board::rom::pins_if_in_loader(port));
        println!("woken: {}", olc_board::rom::wake(port));
        return Ok(());
    }
    let board = parse_id(board).ok_or("bad board id")?;
    let key = ControlKey::load_or_create(&config_folder()?)?;
    let fleet: SharedFleet = Arc::new(Mutex::new(Fleet::new(key, Runner::link_of)?));
    let link = SerialLink::open(port, |line| {
        if !line.starts_with("status") {
            eprintln!("  board says: {line}");
        }
    })
    .map_err(|error| error.to_string())?;
    if what == "who" {
        who(link);
        return Ok(());
    }
    pump(fleet.clone(), link);
    send(&fleet, board, Body::Read(olc_control::Read::Status));
    let deadline = Instant::now() + BOARD_WAIT;
    while beacon(&fleet, board).is_none() {
        if Instant::now() > deadline {
            return Err("no beacon from that board".into());
        }
        thread::sleep(Duration::from_millis(50));
    }
    match what.as_str() {
        "refuse" => refusals(&fleet, board, rest.first().map(String::as_str)),
        "update" => update(&fleet, board, rest.first().ok_or("firmware needed")?),
        "line" => command(&fleet, board, &rest.join(" ")).map(|line| println!("{line:?}")),
        _ => Err("refuse or update".into()),
    }
}
