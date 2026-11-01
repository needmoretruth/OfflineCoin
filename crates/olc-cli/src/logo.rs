//! The spinning coin, for the terminal and the browser.
use olc_board::Args;
use olc_logo::{frame_angle, FRAMES, MAX_FPS};
use std::io::Write;
use std::thread;
use std::time::Duration;

const ROWS: usize = 21;
const COLUMNS: usize = 42;
const FRAME_TIME: Duration = Duration::from_millis(1_000 / MAX_FPS as u64);

pub fn frame(index: usize, rows: usize, columns: usize) -> String {
    let angle = frame_angle(index);
    let mut text = String::with_capacity((columns + 1) * rows);
    for row in 0..rows {
        olc_logo::ascii_row(angle, row, rows, columns, |character| text.push(character));
        text.push('\n');
    }
    text
}

/// Every frame of one turn, small enough for the browser's header.
pub fn frames_for_page() -> Vec<String> {
    (0..FRAMES).map(|index| frame(index, 11, 22)).collect()
}

pub fn run(args: &Args) -> Result<(), String> {
    if args.has("once") {
        print!("{}", frame(1, ROWS, COLUMNS));
        return Ok(());
    }
    let mut out = std::io::stdout();
    write!(out, "\x1b[2J").map_err(|error| error.to_string())?;
    for index in (0..FRAMES).cycle() {
        let text = frame(index, ROWS, COLUMNS);
        // Back to the top left corner. Clearing first would flicker at this pace.
        write!(out, "\x1b[H{text}").map_err(|error| error.to_string())?;
        out.flush().map_err(|error| error.to_string())?;
        thread::sleep(FRAME_TIME);
    }
    Ok(())
}
