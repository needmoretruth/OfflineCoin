#!/usr/bin/env python3
"""Draws the spinning coin as text, at 30 frames a second, for the top of the README.

From the repo root, after `pip install pillow`:
  cargo run --release -q -p olc-logo --example shades -- 90 42 78 > shades.bin
  python3 tools/logo/spin_gif.py shades.bin docs/images/coin.gif
The coin's own 3D renderer gives the light on each cell. A cell becomes a block of metal with a
letter on it, and the letter's weight and grey follow that light. The page shows through the rest.
"""
import sys
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

FRAMES, ROWS, COLUMNS = 90, 42, 78
FONT = "crates/olc-app/fonts/PlexMono-Regular.ttf"
LETTERS = "`.'^,:;!i+clxzvunrt1?oaeXYZUCQ0*bdpqmwkhO8%&#MW@"
# A cell as drawn. The frame is halved afterwards, so the letters keep smooth edges.
CELL = (13, 24)
GREYS = 28
# Above 1 pushes the stamp away from the darker face around it.
CONTRAST = 1.5
METAL = (18, 112)
INK = (70, 255)
# Fewer greys in the file make it far smaller, and the eye does not miss them at this size.
STEP = 16
# Three frames of 3/100 s and one of 4 is no even split, so 3, 3, 4 gives exactly 30 a second.
DELAYS = [30, 30, 40]


def mixed(ends, level):
    return round(ends[0] + (ends[1] - ends[0]) * level)


def by_weight(font):
    """The letters from least ink to most, as this font really draws them."""
    box = (CELL[0] * 2, CELL[1] * 2)

    def ink(letter):
        cell = Image.new("L", box, 0)
        ImageDraw.Draw(cell).text((2, 2), letter, font=font, fill=255)
        return sum(cell.tobytes())

    return sorted(LETTERS, key=ink)


def tiles(font, order):
    """A finished cell for each grey, so that a frame is only pasting."""
    made = {}
    for grey in range(GREYS):
        level = grey / (GREYS - 1)
        metal = mixed(METAL, level)
        ink = mixed(INK, level)
        letter = order[min(int(level * len(order)), len(order) - 1)]
        tile = Image.new("L", CELL, metal)
        ImageDraw.Draw(tile).text(
            (CELL[0] // 2, CELL[1] // 2), letter, font=font, fill=ink, anchor="mm"
        )
        made[grey] = tile
    return made


def frame(cells, made):
    size = (COLUMNS * CELL[0], ROWS * CELL[1])
    grey = Image.new("L", size, 0)
    shape = Image.new("L", (COLUMNS, ROWS), 0)
    for row in range(ROWS):
        for column in range(COLUMNS):
            value = cells[row * COLUMNS + column]
            if value == 0:
                continue
            light = (value - 1) / 254
            light = min(max(0.5 + (light - 0.42) * CONTRAST, 0), 1)
            spot = (column * CELL[0], row * CELL[1])
            grey.paste(made[int(light * (GREYS - 1))], spot)
            shape.putpixel((column, row), 255)
    final = (size[0] // 2, size[1] // 2)
    return grey.resize(final, Image.LANCZOS), shape.resize(final, Image.NEAREST)


def paletted(grey, shape):
    """Index 255 is the page. The rest of the palette is the grey ramp itself."""
    levels = grey.point(lambda value: value // STEP)
    picture = Image.composite(levels, Image.new("L", grey.size, 255), shape).convert("P")
    picture.putpalette([channel for level in range(256) for channel in [min(level * STEP + STEP // 2, 255)] * 3])
    return picture


def main():
    shades = Path(sys.argv[1]).read_bytes()
    out = Path(sys.argv[2])
    out.parent.mkdir(parents=True, exist_ok=True)
    font = ImageFont.truetype(FONT, CELL[0] / 0.6)
    made = tiles(font, by_weight(font))
    each = ROWS * COLUMNS
    pictures = [
        paletted(*frame(shades[index * each : (index + 1) * each], made))
        for index in range(FRAMES)
    ]
    pictures[0].save(
        out,
        save_all=True,
        append_images=pictures[1:],
        duration=[DELAYS[index % 3] for index in range(FRAMES)],
        loop=0,
        transparency=255,
        disposal=2,
        optimize=False,
    )


main()
