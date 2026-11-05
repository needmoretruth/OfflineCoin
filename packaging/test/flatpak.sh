#!/usr/bin/env bash
# Installs the real Flatpak bundle as a software center does, which fetches its runtime from Flathub
# by itself, starts it from its menu entry, mines, opens a serial port from inside the sandbox, and
# removes it. Usage: flatpak.sh <file.flatpak> <olc tar.gz> <folder for screenshots>
# Uses the screen in DISPLAY, or starts one.
set -euo pipefail

bundle=$(realpath "$1")
terminal=$(realpath "$2")
shots=$(realpath "$3")
here=$(cd "$(dirname "$0")" && pwd)
app=io.github.needmoretruth.OfflineCoin
wait_seconds=120

for tool in flatpak dbus-run-session Xvfb xwininfo import desktop-file-validate; do
  if ! command -v "$tool" >/dev/null; then
    sudo apt-get update -q
    sudo apt-get install -y -q flatpak dbus xvfb x11-utils imagemagick desktop-file-utils
    break
  fi
done
# Flatpak talks to the session's message bus, which a desktop has and a test machine lacks.
if [ -z "${DBUS_SESSION_BUS_ADDRESS:-}" ]; then
  exec dbus-run-session -- "$0" "$@"
fi

xvfb=
if [ -z "${DISPLAY:-}" ]; then
  export DISPLAY=:99
  Xvfb "$DISPLAY" -screen 0 1280x800x24 -ac -nolisten tcp &
  xvfb=$!
  sleep 2
fi
trap '[ -z "$xvfb" ] || kill "$xvfb" 2>/dev/null || true' EXIT
export LIBGL_ALWAYS_SOFTWARE=1
unset WAYLAND_DISPLAY

# No Flathub set up beforehand: the bundle names it, and installing adds it.
flatpak install --user --noninteractive -y "$bundle"

exports=${XDG_DATA_HOME:-$HOME/.local/share}/flatpak/exports/share
entry=$exports/applications/$app.desktop
desktop-file-validate "$entry"
for size in 48x48 128x128 256x256 512x512; do
  test -f "$exports/icons/hicolor/$size/apps/$app.png"
done
flatpak run --command=espflash "$app" --version

window() {
  [[ $(xwininfo -root -tree) == *'"OLC Core"'* ]]
}

# What the app menu runs, read from the entry itself.
open_app() {
  local start
  start=$(sed -n 's/^Exec=//p' "$entry" | sed 's/ @@[^ ]*//g; s/ %[a-zA-Z]//g')
  $start &
  opened=$!
  local waited=0
  until window; do
    sleep 1
    waited=$((waited + 1))
    if [ "$waited" -ge "$wait_seconds" ]; then
      echo "No window after $wait_seconds seconds." >&2
      return 1
    fi
  done
}

close_app() {
  flatpak kill "$app"
  local gone=0
  while window && [ "$gone" -lt 20 ]; do
    sleep 1
    gone=$((gone + 1))
  done
  # Only the app: the screen this test started runs on.
  wait "$opened" || true
}

open_app
sleep 8
import -window root "$shots/first-start.png"
close_app

# The sandbox has its own /tmp, so the test's folder is handed in, for this test alone.
work=$HOME/olc-flatpak-test
mkdir -p "$work/olc"
printf '#!/bin/sh\nexec flatpak run --filesystem=%s --command=olc-core %s "$@"\n' "$work" "$app" \
  > "$work/olc-core"
chmod +x "$work/olc-core"
tar -xzf "$terminal" -C "$work/olc" --strip-components=1
TMPDIR=$work python3 "$here/mine.py" "$work/olc-core" "$work/olc/olc" "$shots"

# A node outside the sandbox reaches the app's node inside it over the computer's network address,
# as another computer on the network would, and takes the blocks it mines. The node listens on every
# address, as the app starts it when the network may reach it.
address=$(hostname -I | awk '{print $1}')
"$work/olc/olc" --data "$work/empty" save-coin "$work/coin.json"
# The node ends when its input closes, so the input stays open until the test is done with it.
mkfifo "$work/input"
"$work/olc-core" node --data "$work/app" --wallet "$work/app" --coin "$work/coin.json" \
  --bind 0.0.0.0 --port 17321 --mine --cap 0 < "$work/input" > "$work/app-node.log" &
node=$!
exec 3> "$work/input"
joined=$(timeout 120 "$work/olc/olc" node --data "$work/outside" --port 17322 \
  --connect "$address:17321" 2>&1 | tee "$work/outside.log" \
  | grep -m 1 -E '^height [1-9][0-9]* peers [1-9]' || true)
echo quit >&3
exec 3>&-
wait "$node" || true
if [ -z "$joined" ]; then
  echo "No node outside the sandbox joined the app's node at $address." >&2
  tail -n 20 "$work/outside.log" >&2
  tail -c 1500 "$work/app-node.log" >&2
  exit 1
fi
echo "A node outside the sandbox joined the app's node at $address:17321: $joined" \
  | tee "$shots/peers.txt"
rm -rf "$work"

# The app's own Start mining, already pressed. Its folder is the person's own, outside the sandbox.
mkdir -p "$HOME/.olc"
printf '{"setup_done":true,"mine_on_start":true,"hash_cap":0}' > "$HOME/.olc/core.json"
open_app
sleep 30
import -window root "$shots/mining.png"
close_app

# A board is a serial port. Inside the sandbox the person reaches the same ports as outside, with
# the same permission: a member of the port's group opens one.
port=$(find /dev -maxdepth 1 -name 'ttyS[0-9]*' | sort | head -n 1)
group=$(stat -c %G "$port")
flatpak run --command=sh "$app" -c "test -d /sys/class/tty && test -c $port"
opens="import os; os.close(os.open('$port', os.O_RDWR | os.O_NOCTTY | os.O_NONBLOCK))"
before=refused
if flatpak run --command=python3 "$app" -c "$opens" 2>/dev/null; then
  before=opened
fi
# Joining the group takes a new login, which sudo gives: the person's own groups, read afresh.
sudo usermod -aG "$group" "$(id -un)"
sudo --preserve-env=HOME,XDG_RUNTIME_DIR,DBUS_SESSION_BUS_ADDRESS -u "$(id -un)" \
  flatpak run --command=python3 "$app" -c "$opens"
echo "Before joining $group, $port was $before inside the sandbox. After, it opened." \
  | tee "$shots/serial.txt"

flatpak uninstall --user --noninteractive -y "$app"
test ! -e "$entry"
test -f "$HOME/.olc/node/wallet.json"
echo 'Removed the app, and the wallet stayed.' | tee "$shots/removed.txt"
