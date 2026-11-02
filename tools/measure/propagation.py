#!/usr/bin/env python3
"""Time from `mined` on one cable to `block` on the other, with no app: propagation.py PORT_A PORT_B MINUTES [LINKS]."""
import re
import statistics
import sys
import threading
import time

import boards


def listen(cable, name, seen, stop):
    tail = ""
    while not stop.is_set():
        text = tail + cable.hear(0.01)
        lines = text.split("\n")
        tail = lines.pop()
        at = time.time()
        for line in lines:
            found = re.fullmatch(r"(mined|block) (\d+)\r?", line)
            if found:
                seen.setdefault(int(found[2]), {}).setdefault(found[1] + name, at)


def main():
    port_a, port_b, minutes = sys.argv[1], sys.argv[2], float(sys.argv[3])
    a, b = boards.Cable(port_a), boards.Cable(port_b)
    if len(sys.argv) > 4:
        a.say(f"links {sys.argv[4]}")
        b.say(f"links {sys.argv[4]}")
    seen, stop = {}, threading.Event()
    threads = [threading.Thread(target=listen, args=(cable, name, seen, stop))
               for cable, name in ((a, "a"), (b, "b"))]
    for thread in threads:
        thread.start()
    time.sleep(minutes * 60)
    stop.set()
    for thread in threads:
        thread.join()
    delays = []
    for height, times in sorted(seen.items()):
        for found, heard in (("mineda", "blockb"), ("minedb", "blocka")):
            if found in times and heard in times:
                delays.append((times[heard] - times[found]) * 1000)
                print(height, found[-1], "to", heard[-1], round(delays[-1]), "ms", flush=True)
    if delays:
        print(f"blocks {len(delays)} median {statistics.median(delays):.0f} ms "
              f"worst {max(delays):.0f} ms of {len(seen)} heights")


if __name__ == "__main__":
    main()
