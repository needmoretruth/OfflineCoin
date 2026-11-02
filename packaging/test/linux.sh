#!/usr/bin/env bash
# Installs the real .deb the way a double click does, starts it from its menu entry, mines, and then
# does the same on bare older systems where only the package and its Depends are present.
# Usage: linux.sh <file.deb> <olc tar.gz> <folder for screenshots>
set -euo pipefail

deb=$(realpath "$1")
terminal=$(realpath "$2")
shots=$(realpath "$3")
here=$(cd "$(dirname "$0")" && pwd)
screen=:99
wait_seconds=60

sudo apt-get update -q
sudo apt-get install -y -q xvfb x11-utils imagemagick desktop-file-utils
Xvfb "$screen" -screen 0 1280x800x24 -ac -nolisten tcp &
xvfb=$!
trap 'kill "$xvfb" 2>/dev/null || true' EXIT
sleep 2
export DISPLAY=$screen LIBGL_ALWAYS_SOFTWARE=1
unset WAYLAND_DISPLAY

sudo apt-get install -y -q "$deb"

entry=/usr/share/applications/olc-core.desktop
desktop-file-validate "$entry"
for size in 48x48 128x128 256x256 512x512; do
  test -f "/usr/share/icons/hicolor/$size/apps/olc-core.png"
done
/usr/lib/olc-core/espflash --version
rule=/usr/lib/udev/rules.d/70-olc-boards.rules
test -f "$rule"
udevadm verify "$rule"
first=$(find /usr/lib/udev/rules.d /etc/udev/rules.d -name '73-seat-late.rules' | head -n 1)
[[ "$(basename "$rule")" < "$(basename "$first")" ]]

window() {
  [[ $(xwininfo -root -tree) == *'"OLC Core"'* ]]
}

wait_for_window() {
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

# What the app menu runs, read from the entry itself.
start=$(sed -n 's/^Exec=//p' "$entry")
$start &
app=$!
wait_for_window
sleep 8
import -window root "$shots/first-start.png"
xwininfo -root -tree | grep '"OLC Core"'
kill "$app"
wait "$app" || true

mkdir -p "$shots/olc"
tar -xzf "$terminal" -C "$shots/olc" --strip-components=1
python3 "$here/mine.py" "$(command -v olc-core)" "$shots/olc/olc" "$shots"
rm -rf "$shots/olc"

# The app's own Start mining, already pressed.
mkdir -p "$HOME/.olc"
printf '{"setup_done":true,"mine_on_start":true,"hash_cap":0}' > "$HOME/.olc/core.json"
$start &
app=$!
wait_for_window
sleep 30
import -window root "$shots/mining.png"
kill "$app"
wait "$app" || true

# Only the package and what it asks for: nothing the runner happens to have can help it here. The
# screen stays outside, so no test tool brings a library in.
bare() {
  local image=$1
  local name=${image//[:.]/-}
  docker run -d --name "$name" --ipc=host -e DISPLAY=$screen -e LIBGL_ALWAYS_SOFTWARE=1 \
    -e DEBIAN_FRONTEND=noninteractive -v /tmp/.X11-unix:/tmp/.X11-unix:ro -v "$deb:/olc-core.deb:ro" \
    "$image" sh -c 'apt-get update -q && apt-get install -y -q --no-install-recommends /olc-core.deb && exec olc-core'
  local waited=0
  until window; do
    sleep 2
    waited=$((waited + 2))
    if [ "$waited" -ge 240 ] || [ "$(docker inspect -f '{{.State.Running}}' "$name")" != true ]; then
      docker logs --tail 40 "$name" >&2
      return 1
    fi
  done
  sleep 8
  import -window root "$shots/bare-$name.png"
  docker exec "$name" sh -c 'ldd --version | head -n 1'
  docker rm -f "$name"
  local gone=0
  while window && [ "$gone" -lt 10 ]; do
    sleep 1
    gone=$((gone + 1))
  done
}

bare ubuntu:22.04
bare debian:12
