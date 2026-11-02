#!/usr/bin/env python3
"""Runs `olc update` on a board and prints each stage with its second: update_time.py BOARD_NAME [SECONDS]."""
import subprocess
import sys
import time

import boards


def main():
    boards.needs(1, __doc__)
    name = sys.argv[1]
    limit = float(sys.argv[2]) if len(sys.argv) > 2 else 600
    start = time.time()
    update = subprocess.Popen([boards.OLC, "update", name], stdout=subprocess.PIPE, text=True)
    for stage in update.stdout:
        print(round(time.time() - start), stage.strip(), flush=True)
        if time.time() - start > limit:
            update.kill()
            break
    print("ended", update.wait(), "after", round(time.time() - start), "seconds", flush=True)
    print("build", boards.status(name).get("build"), flush=True)


if __name__ == "__main__":
    main()
