//! The coin's brightness as raw bytes, for drawing it somewhere this crate does not reach.
//! Usage: shades FRAMES ROWS COLUMNS. One byte a cell: 0 is off the coin, 1 to 255 is on it.
use olc_logo::Spin;
use std::io::Write;

fn main() -> std::io::Result<()> {
    let number = |index: usize| {
        let word = std::env::args().nth(index);
        word.and_then(|word| word.parse::<usize>().ok()).expect("shades FRAMES ROWS COLUMNS")
    };
    let (frames, rows, columns) = (number(1), number(2), number(3));
    let mut out = std::io::stdout().lock();
    for frame in 0..frames {
        let spin = Spin::new(std::f32::consts::TAU * frame as f32 / frames as f32);
        for row in 0..rows {
            let y = 1.0 - (row as f32 + 0.5) * 2.0 / rows as f32;
            let cells: Vec<u8> = (0..columns)
                .map(|column| {
                    let x = (column as f32 + 0.5) * 2.0 / columns as f32 - 1.0;
                    spin.shade(x, y).map_or(0, |light| 1 + (light.clamp(0.0, 1.0) * 254.0) as u8)
                })
                .collect();
            out.write_all(&cells)?;
        }
    }
    Ok(())
}
