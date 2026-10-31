//! A 128x64 SH1106 OLED on I2C. The screen is optional: the board runs without one, and picks one
//! up within a second of it being plugged in. Only rows that changed are sent, so a redraw costs
//! the miner a few milliseconds.
use crate::hangul;
use embedded_graphics::mono_font::ascii::{FONT_10X20, FONT_5X8, FONT_6X10, FONT_7X13};
use embedded_graphics::mono_font::{MonoFont, MonoTextStyle};
use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::prelude::*;
use embedded_graphics::text::{Baseline, Text};
use esp_hal::i2c::master::I2c;
use esp_hal::Blocking;

const ADDRESS: u8 = 0x3C;
pub const WIDTH: usize = 128;
pub const PAGES: usize = 8;
const HEIGHT: usize = PAGES * 8;
/// The SH1106 has 132 columns of memory and the panel starts at the third.
const COLUMN_OFFSET: u8 = 2;

const COMMAND: u8 = 0x00;
const DATA: u8 = 0x40;
const STARTUP: [u8; 24] = [
    COMMAND, 0xAE, 0xD5, 0x80, 0xA8, 0x3F, 0xD3, 0x00, 0x40, 0x8D, 0x14, 0x20, 0x00, 0xA1, 0xC8,
    0xDA, 0x12, 0x81, 0xCF, 0xD9, 0xF1, 0xDB, 0x40, 0xA4,
];
const SWITCH_ON: [u8; 3] = [COMMAND, 0xA6, 0xAF];

#[derive(Clone, Copy)]
pub enum Font {
    Small,
    Regular,
    Reading,
    Big,
}

impl Font {
    fn latin(self) -> &'static MonoFont<'static> {
        match self {
            Font::Small => &FONT_5X8,
            Font::Regular => &FONT_6X10,
            Font::Reading => &FONT_7X13,
            Font::Big => &FONT_10X20,
        }
    }

    fn hangul(self) -> &'static [hangul::Glyph] {
        match self {
            Font::Reading => hangul::LARGE,
            _ => hangul::SMALL,
        }
    }
}

type Pixels = [u8; WIDTH * PAGES];

pub struct Screen {
    bus: I2c<'static, Blocking>,
    pixels: Pixels,
    /// What the panel holds, or `None` when no panel has answered yet.
    shown: Option<Pixels>,
}

struct Canvas<'a>(&'a mut Pixels);

impl OriginDimensions for Canvas<'_> {
    fn size(&self) -> Size {
        Size::new(WIDTH as u32, HEIGHT as u32)
    }
}

impl DrawTarget for Canvas<'_> {
    type Color = BinaryColor;
    type Error = core::convert::Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for Pixel(point, color) in pixels {
            if let (Ok(x @ 0..WIDTH), Ok(y @ 0..HEIGHT)) =
                (usize::try_from(point.x), usize::try_from(point.y))
            {
                let bit = 1 << (y % 8);
                let byte = &mut self.0[y / 8 * WIDTH + x];
                if color.is_on() {
                    *byte |= bit;
                } else {
                    *byte &= !bit;
                }
            }
        }
        Ok(())
    }
}

fn glyph(character: char, font: Font) -> Option<&'static hangul::Glyph> {
    let table = font.hangul();
    let found = table.binary_search_by_key(&character, |entry| entry.0);
    found.ok().map(|index| &table[index])
}

impl Screen {
    pub fn new(bus: I2c<'static, Blocking>) -> Self {
        Self {
            bus,
            pixels: [0; WIDTH * PAGES],
            shown: None,
        }
    }

    pub fn is_connected(&self) -> bool {
        self.shown.is_some()
    }

    pub fn clear(&mut self) {
        self.pixels.fill(0);
    }

    pub fn pixel(&mut self, x: i32, y: i32, on: bool) {
        let _ = Canvas(&mut self.pixels).draw_iter([Pixel(
            Point::new(x, y),
            if on {
                BinaryColor::On
            } else {
                BinaryColor::Off
            },
        )]);
    }

    /// Draws text from its top left corner and returns where it ended. Korean letters come from
    /// the glyph table, everything else from the Latin font.
    pub fn text(&mut self, mut x: i32, y: i32, text: &str, font: Font) -> i32 {
        let style = MonoTextStyle::new(font.latin(), BinaryColor::On);
        let advance = font.latin().character_size.width as i32;
        let mut letter = [0u8; 4];
        for character in text.chars() {
            if character.is_ascii() {
                let text = character.encode_utf8(&mut letter);
                let position = Point::new(x, y);
                let mut canvas = Canvas(&mut self.pixels);
                let _ = Text::with_baseline(text, position, style, Baseline::Top).draw(&mut canvas);
                x += advance;
            } else {
                x += i32::from(self.hangul(x, y, character, font));
            }
        }
        x
    }

    pub fn text_right(&mut self, y: i32, text: &str, font: Font) {
        let advance = font.latin().character_size.width as i32;
        let width: i32 = text
            .chars()
            .map(|character| {
                if character.is_ascii() {
                    advance
                } else {
                    glyph(character, font).map_or(advance, |found| i32::from(found.1))
                }
            })
            .sum();
        self.text(WIDTH as i32 - width, y, text, font);
    }

    fn hangul(&mut self, x: i32, y: i32, character: char, font: Font) -> u8 {
        let Some(&(_, advance, rows)) = glyph(character, font) else {
            return 0;
        };
        for (row, bits) in rows.iter().enumerate() {
            for column in 0..16 {
                if bits & (0x8000 >> column) != 0 {
                    self.pixel(x + column, y + row as i32 - 1, true);
                }
            }
        }
        advance
    }

    /// A thin bar across the panel, filled from the left.
    pub fn bar(&mut self, top: i32, height: i32, filled: f32) {
        let last = WIDTH as i32 - 1;
        let reach = (last - 1) as f32 * filled.clamp(0.0, 1.0);
        for y in top..top + height {
            for x in 0..=last {
                let border = y == top || y == top + height - 1 || x == 0 || x == last;
                self.pixel(x, y, border || (x as f32) <= reach);
            }
        }
    }

    /// Sends the rows that changed. Any failure forgets the panel, so the next frame starts it
    /// again from nothing, which is what a panel that was unplugged needs.
    pub fn flush(&mut self) {
        if self.shown.is_none() && self.start().is_err() {
            return;
        }
        let Some(mut shown) = self.shown.take() else {
            return;
        };
        let mut data = [DATA; WIDTH + 1];
        for page in 0..PAGES {
            let row = &self.pixels[page * WIDTH..][..WIDTH];
            let old = &mut shown[page * WIDTH..][..WIDTH];
            let changed = |(new, old): (&u8, &u8)| new != old;
            let Some(first) = row.iter().zip(old.iter()).position(changed) else {
                continue;
            };
            let tail = row
                .iter()
                .rev()
                .zip(old.iter().rev())
                .position(changed)
                .unwrap_or(0);
            let end = WIDTH - tail;
            old[first..end].copy_from_slice(&row[first..end]);
            let size = end - first;
            data[1..=size].copy_from_slice(&row[first..end]);
            if self.select(page, first).is_err() || self.bus.write(ADDRESS, &data[..=size]).is_err()
            {
                return;
            }
        }
        self.shown = Some(shown);
    }

    fn start(&mut self) -> Result<(), ()> {
        self.bus.write(ADDRESS, &STARTUP).map_err(|_| ())?;
        let blank = [DATA; WIDTH + 1];
        for page in 0..PAGES {
            self.select(page, 0)?;
            let mut blank = blank;
            blank[1..].fill(0);
            self.bus.write(ADDRESS, &blank).map_err(|_| ())?;
        }
        self.bus.write(ADDRESS, &SWITCH_ON).map_err(|_| ())?;
        self.shown = Some([0; WIDTH * PAGES]);
        Ok(())
    }

    fn select(&mut self, page: usize, column: usize) -> Result<(), ()> {
        let column = column as u8 + COLUMN_OFFSET;
        let select = [
            COMMAND,
            0xB0 | page as u8,
            column & 0x0F,
            0x10 | column >> 4,
        ];
        self.bus.write(ADDRESS, &select).map_err(|_| ())
    }

    /// Reads one row back out of the panel's own memory, to prove what it holds.
    pub fn read_row(&mut self, page: usize, row: &mut [u8; WIDTH]) -> Result<(), ()> {
        self.select(page, 0)?;
        // The first byte the chip returns after setting an address is a dummy.
        let mut bytes = [0u8; WIDTH + 1];
        self.bus
            .write_read(ADDRESS, &[DATA], &mut bytes)
            .map_err(|_| ())?;
        row.copy_from_slice(&bytes[1..]);
        Ok(())
    }

    pub fn row(&self, page: usize) -> &[u8] {
        &self.pixels[page * WIDTH..][..WIDTH]
    }
}
