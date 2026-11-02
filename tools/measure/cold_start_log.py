"""Logs what each board does on a cold plug, from the repo root: cold_start_log.py MAC_END [MAC_END...]

Waits on the by-id paths of the named boards. Nothing opens a port until it re-appears, so nothing
disturbs the start. For each arrival it records the first bytes the chip sends, asks the ROM
loader whether it is there, and prints one verdict line:
  ROM-WAIT   the chip's ROM sits in its download wait. Our code and the bootloader never ran.
  STARTED    our program answers. This plug did not stall.
  AFTER-ROM  the ROM left, and nothing of ours answers: a stop in the bootloader or very early.
A ROM wait is then ended with the same no-hands wake the app uses.
"""
import glob, os, subprocess, sys, time
names = sys.argv[1:]
out = open('cold-start.log', 'a', buffering=1)
def say(*w):
    line = time.strftime('%H:%M:%S ') + ' '.join(str(x) for x in w)
    print(line, flush=True); out.write(line + '\n')
def path(name):
    found = glob.glob(f'/dev/serial/by-id/*{name}-if00')
    return found[0] if found else None
def probe(name, port):
    t0 = time.time(); fd = None
    for _ in range(300):
        try:
            subprocess.run(['stty', '-F', port, '115200', 'raw', '-echo', '-hupcl', 'min', '0', 'time', '1'], check=True, capture_output=True)
            fd = os.open(port, os.O_RDWR | os.O_NOCTTY); break
        except Exception:
            time.sleep(0.02)
    if fd is None:
        say(name, 'could not open'); return
    say(name, 'opened after', round(time.time() - t0, 3))
    data = b''
    while time.time() - t0 < 2.5:
        data += os.read(fd, 4096)
    os.close(fd)
    say(name, 'first bytes', len(data), repr(data[:500]))
    rom = {}
    for label, addr in (('strap', '0x60004038'), ('rtc-reset', '0x60008038')):
        r = subprocess.run(['esptool', '--chip', 'esp32s3', '--port', port, '--before', 'no-reset', '--after', 'no-reset', '--no-stub', 'read-mem', addr], capture_output=True, text=True)
        rom[label] = r.returncode == 0
        say(name, label, r.returncode, ' | '.join((r.stdout + r.stderr).strip().splitlines()[-2:])[:200])
    if all(rom.values()) or b'waiting for download' in data:
        say(name, 'VERDICT ROM-WAIT: the ROM holds it. No code of ours ran.')
        r = subprocess.run(['target/release/examples/boardcheck', port, 'x', 'loader'], capture_output=True, text=True)
        say(name, 'wake', r.stdout.strip()[:120])
        return
    # Past the ROM. A chain load takes minutes, but the board prints as soon as it runs.
    fd = os.open(port, os.O_RDWR | os.O_NOCTTY); later = b''; t = time.time()
    while time.time() - t < 25 and len(later) < 40:
        later += os.read(fd, 4096)
    os.close(fd)
    say(name, 'later bytes', len(later), repr(later[:200]))
    if later or (data and b'ESP-ROM' not in data):
        say(name, 'VERDICT STARTED: our program is talking.')
    else:
        say(name, 'VERDICT AFTER-ROM: the ROM left and nothing answers.')
seen = {name: path(name) for name in names}
say('watching', seen)
while True:
    for name in names:
        now = path(name)
        if now and not seen[name]:
            say(name, 'appeared'); seen[name] = now; probe(name, now)
        elif not now and seen[name]:
            say(name, 'gone'); seen[name] = None
    time.sleep(0.01)
