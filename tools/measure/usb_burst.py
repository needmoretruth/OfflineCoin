#!/usr/bin/env python3
"""Counts bytes lost on the way into a board over USB: usb_burst.py PORT [ROUNDS]."""
import os
import re
import subprocess
import sys
import time

PAD = 216  # a typed line of 223 bytes, the size of a small node message
PATTERNS = ((1, 0.05, 200), (8, 0.2, 400), (8, 0.02, 400), (1, 0.005, 400), (40, 0.2, 800))


def round_of(fd, burst, gap, lines):
    line = b"\n\n\n\nlength " + b"z" * PAD + b"\n"
    expected = len(line.strip())
    got = b""
    sent = 0
    while sent < lines:
        os.write(fd, line * burst)
        sent += burst
        end = time.time() + gap
        while time.time() < end:
            got += os.read(fd, 65536)
            time.sleep(0.002)
    end = time.time() + 2
    while time.time() < end:
        got += os.read(fd, 65536)
        time.sleep(0.01)
    seen = [int(number) for number in re.findall(rb"length (\d+)", got)]
    wrong = [number for number in seen if number != expected]
    lost = sum(expected - number for number in wrong) + (sent - len(seen)) * expected
    print(f"burst {burst:2} gap {gap}: sent {sent} answered {len(seen)} wrong {len(wrong)} "
          f"bytes lost {lost}", flush=True)
    return len(wrong) + sent - len(seen)


def main():
    path = sys.argv[1]
    rounds = int(sys.argv[2]) if len(sys.argv) > 2 else 3
    subprocess.run(["stty", "-F", path, "115200", "raw", "-echo", "-hupcl", "min", "0", "time", "0"],
                   check=True)
    fd = os.open(path, os.O_RDWR | os.O_NOCTTY)
    damaged = 0
    for _ in range(rounds):
        for burst, gap, lines in PATTERNS:
            damaged += round_of(fd, burst, gap, lines)
    print(f"damaged lines: {damaged}")


if __name__ == "__main__":
    main()
