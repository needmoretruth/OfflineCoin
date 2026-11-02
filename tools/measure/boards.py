"""Shared by the scripts here: lines and status of boards, down their own USB cables.

A cable has one owner, so close OLC Core and stop any `olc` first. A board goes by its port, its
name or its id. OLC_PORTS lists ports by hand, with commas. OLC names the `olc` program.
"""
import glob
import os
import re
import sys
import termios
import time

OLC = os.environ.get("OLC", "olc")
# The same waits as the program's own cable: a board has finished once it stops talking.
ANSWER_WAIT = 2.0
QUIET_AFTER = 0.3
READ_STEP = 0.05
# A board printing as a line arrives can lose the first bytes, and empty lines mean nothing to it.
LINE_START = "\n\n\n\n"
PORT_PATTERNS = ("/dev/serial/by-id/*Espressif*", "/dev/ttyACM*", "/dev/cu.usbmodem*")


def needs(count, usage):
    """Shows the script's usage and ends when it was given fewer than `count` words."""
    if len(sys.argv) <= count:
        print(usage.strip(), file=sys.stderr)
        sys.exit(2)


class Cable:
    def __init__(self, path):
        self.fd = os.open(path, os.O_RDWR | os.O_NOCTTY | os.O_NONBLOCK)
        mode = termios.tcgetattr(self.fd)
        mode[0] = mode[1] = mode[3] = 0
        mode[2] &= ~termios.HUPCL
        termios.tcsetattr(self.fd, termios.TCSANOW, mode)

    def say(self, text):
        os.write(self.fd, (LINE_START + text + "\n").encode())

    def hear(self, seconds):
        got = b""
        end = time.time() + seconds
        while time.time() < end:
            try:
                got += os.read(self.fd, 4096)
            except BlockingIOError:
                time.sleep(0.02)
        return got.decode(errors="replace")


_open = {}
_ports_by_name = {}


def ports():
    """The ports with a board on them, by hand or by their USB name."""
    chosen = os.environ.get("OLC_PORTS")
    if chosen:
        return chosen.split(",")
    for pattern in PORT_PATTERNS:
        found = sorted(glob.glob(pattern))
        if found:
            return found
    return []


def cable_on(port):
    """The one open cable of `port`."""
    if port not in _open:
        _open[port] = Cable(port)
    return _open[port]


def _ask(port, text):
    """The lines a board printed after `text`, without any node message."""
    cable = cable_on(port)
    cable.hear(0)
    cable.say(text)
    heard = ""
    start = last = time.time()
    while time.time() - start < ANSWER_WAIT:
        got = cable.hear(READ_STEP)
        if got:
            heard += got
            last = time.time()
        elif heard and time.time() - last >= QUIET_AFTER:
            break
    return [line.strip() for line in heard.split("\n") if line.strip() and not line.startswith("#")]


def _status_on(port):
    for _ in range(3):
        for said in _ask(port, "status"):
            if said.startswith("status "):
                return dict(re.findall(r"(\w+)=(\S+)", said))
        time.sleep(1)
    return {}


def boards():
    """Every board on a cable, with the fields of its status and its port."""
    found = []
    for port in ports():
        fields = _status_on(port)
        if fields:
            fields["port"] = port
            _ports_by_name[fields.get("id")] = port
            _ports_by_name[fields.get("name")] = port
            found.append(fields)
    return found


def port_of(board):
    """The port of a board named by its port, name or id."""
    if board.startswith("/dev/"):
        return board
    if board not in _ports_by_name:
        boards()
    if board not in _ports_by_name:
        sys.exit(f"No board called {board} on a cable.")
    return _ports_by_name[board]


def line(board, text):
    """Runs a line on a board and returns its answer, or None."""
    return "\n".join(_ask(port_of(board), text)) or None


def status(board):
    """The board's status line as a dictionary."""
    return _status_on(port_of(board))


def wait_for(check, seconds, every=2.0):
    """Polls until check() is true. Returns the seconds it took, or None."""
    start = time.time()
    while time.time() - start < seconds:
        try:
            if check():
                return round(time.time() - start, 1)
        except OSError:
            pass
        time.sleep(every)
    return None
