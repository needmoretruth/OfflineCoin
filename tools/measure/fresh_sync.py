#!/usr/bin/env python3
"""Erases a board's chain but not its keys or settings, and times its sync with no app: fresh_sync.py PORT TARGET_HEIGHT."""
import re
import subprocess
import sys
import time

import boards

CHAIN_START = 0x410000
CHAIN_SIZE = 0x7E0000
LIMIT_SECONDS = 3600


def main():
    port, target = sys.argv[1], int(sys.argv[2])
    subprocess.run(
        ["esptool", "--chip", "esp32s3", "--port", port, "erase-region", hex(CHAIN_START), hex(CHAIN_SIZE)],
        check=True,
    )
    subprocess.run(["esptool", "--chip", "esp32s3", "--port", port, "--after", "hard-reset", "read-mac"], check=True)
    cable = boards.Cable(port)
    start = time.time()
    last = -1
    while time.time() - start < LIMIT_SECONDS:
        cable.say("status")
        found = re.findall(r"status height=(\d+)", cable.hear(3))
        height = int(found[-1]) if found else last
        if height != last:
            print(f"{time.time() - start:.0f} s height {height}", flush=True)
            last = height
        if height >= target:
            print("synced in", round(time.time() - start), "seconds", flush=True)
            return
    print("not synced, last height", last, flush=True)


if __name__ == "__main__":
    main()
