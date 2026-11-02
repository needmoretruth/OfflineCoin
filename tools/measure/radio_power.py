#!/usr/bin/env python3
"""Signal each board hears with all boards at one radio power, in turn: radio_power.py DBM [DBM...]."""
import statistics
import sys
import time

import boards


def main():
    boards.needs(1, __doc__)
    ids = [board["id"] for board in boards.boards()]
    for power in sys.argv[1:]:
        for board in ids:
            boards.line(board, f"power {power}")
        time.sleep(30)
        values = {board: [] for board in ids}
        for _ in range(12):
            for board in ids:
                value = boards.status(board).get("rssi")
                if value:
                    values[board].append(int(value))
            time.sleep(4)
        print(power, "dBm:", {board[-4:]: (statistics.median(found), min(found))
                              for board, found in values.items() if found}, flush=True)


if __name__ == "__main__":
    main()
