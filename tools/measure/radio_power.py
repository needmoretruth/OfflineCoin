#!/usr/bin/env python3
"""Signal between boards at each radio power: radio_power.py SENDER LISTENER DBM [DBM...]."""
import statistics
import sys
import time

import boards


def main():
    sender, listener = sys.argv[1], sys.argv[2]
    for power in sys.argv[3:]:
        boards.line(sender, f"power {power}")
        time.sleep(30)
        values = []
        for _ in range(12):
            value = boards.status(listener).get("rssi")
            if value:
                values.append(int(value))
            time.sleep(5)
        print(f"{power} dBm: rssi median {statistics.median(values)} "
              f"low {min(values)} high {max(values)} readings {len(values)}", flush=True)


if __name__ == "__main__":
    main()
