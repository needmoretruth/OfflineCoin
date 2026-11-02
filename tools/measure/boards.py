"""Shared by the scripts here: lines and status of boards, through a running `olc app`."""
import json
import os
import re
import termios
import time
import urllib.request


def _config():
    return os.environ.get("OLC_CONFIG") or os.path.expanduser("~/.config/olc")


def _base():
    with open(os.path.join(_config(), "app.port"), encoding="ascii") as file:
        return "http://127.0.0.1:" + file.read().strip()


def _call(path, body=None):
    base = _base()
    data = None if body is None else json.dumps(body).encode()
    request = urllib.request.Request(
        base + path, data=data, headers={"Origin": base, "Content-Type": "application/json"})
    with urllib.request.urlopen(request, timeout=30) as reply:
        return json.load(reply)


def boards():
    """Every board the app knows: name, id, kind, online, height, hashrate, build, limit."""
    return _call("/api/boards")["boards"]


def line(board, text):
    """Runs a line on a board and returns its answer, or None."""
    return _call("/api/board", {"id": board, "line": text}).get("answer")


def start_update(board):
    """Asks the app to update a board to the build it carries."""
    return _call("/api/update", {"id": board})


def status(board):
    """The board's status line as a dictionary."""
    for _ in range(3):
        answer = line(board, "status")
        if answer:
            return dict(re.findall(r"(\w+)=(\S+)", answer))
        time.sleep(1)
    return {}


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


class Cable:
    def __init__(self, path):
        self.fd = os.open(path, os.O_RDWR | os.O_NOCTTY | os.O_NONBLOCK)
        mode = termios.tcgetattr(self.fd)
        mode[0] = mode[1] = mode[3] = 0
        mode[2] &= ~termios.HUPCL
        termios.tcsetattr(self.fd, termios.TCSANOW, mode)

    def say(self, text):
        os.write(self.fd, b"\n\n" + text.encode() + b"\n")

    def hear(self, seconds):
        got = b""
        end = time.time() + seconds
        while time.time() < end:
            try:
                got += os.read(self.fd, 4096)
            except BlockingIOError:
                time.sleep(0.02)
        return got.decode(errors="replace")
