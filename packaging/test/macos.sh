#!/usr/bin/env bash
# Opens the real disk image, copies the app to Applications as the drag does, starts it, and mines.
# Usage: macos.sh <file.dmg> <olc tar.gz> <folder for screenshots>
set -euo pipefail

image=$1
terminal=$2
shots=$3
here=$(cd "$(dirname "$0")" && pwd)
app="/Applications/OLC Core.app"
mounted=$(mktemp -d)
wait_seconds=60

hdiutil attach "$image" -nobrowse -readonly -mountpoint "$mounted"
test "$(readlink "$mounted/Applications")" = /Applications
ditto "$mounted/OLC Core.app" "$app"
hdiutil detach "$mounted"

codesign --verify --deep --strict "$app"
machine=$(uname -m)
file "$app/Contents/MacOS/olc-core" | grep -q "$machine"

# The window's number, once the app has one on the screen. Asks Quartz, so it needs no permission.
window_number() {
  swift "$here/window.swift" "OLC Core"
}

wait_for_window() {
  local waited=0 number=""
  until number=$(window_number) && [ -n "$number" ]; do
    sleep 1
    waited=$((waited + 1))
    if [ "$waited" -ge "$wait_seconds" ]; then
      echo "No window after $wait_seconds seconds." >&2
      return 1
    fi
  done
  echo "$number"
}

shoot() {
  local name=$1 number=$2
  screencapture -x "$shots/$name.png"
  screencapture -x -o -l "$number" "$shots/$name-window.png"
}

close_app() {
  pkill -x olc-core
  sleep 3
}

open "$app"
number=$(wait_for_window)
sleep 8
shoot first-start "$number"
close_app

mkdir -p "$shots/olc"
tar -xzf "$terminal" -C "$shots/olc" --strip-components=1
python3 "$here/mine.py" "$app/Contents/MacOS/olc-core" "$shots/olc/olc" "$shots"
rm -rf "$shots/olc"

# The app's own Start mining, already pressed.
data="$HOME/Library/Application Support/OLC"
mkdir -p "$data"
printf '{"setup_done":true,"mine_on_start":true,"hash_cap":0}' > "$data/core.json"
open "$app"
number=$(wait_for_window)
sleep 30
shoot mining "$number"
close_app
