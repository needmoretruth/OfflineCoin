#!/usr/bin/env bash
# Installs the real .rpm on Fedora, or the Arch Linux package on Arch Linux, in a container of that
# system with only the package and what it asks for, as the software center or pacman does. Then it
# starts the app from its menu entry on a screen outside the container, mines, checks the board
# rule, and removes it. An .rpm is also installed and opened on openSUSE Tumbleweed and AlmaLinux.
# Usage: distro.sh <package> <olc tar.gz> <folder for screenshots>
# Uses the screen in DISPLAY, or starts one.
set -euo pipefail

package=$(realpath "$1")
terminal=$(realpath "$2")
shots=$(realpath "$3")
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
engine=${CONTAINER:-docker}
name=$(basename "$package")
container=olc-try-$$
wait_seconds=240

for tool in Xvfb xwininfo import; do
  if ! command -v "$tool" >/dev/null; then
    sudo apt-get update -q
    sudo apt-get install -y -q xvfb x11-utils imagemagick
    break
  fi
done
xvfb=
if [ -z "${DISPLAY:-}" ]; then
  export DISPLAY=:99
  Xvfb "$DISPLAY" -screen 0 1280x800x24 -ac -nolisten tcp &
  xvfb=$!
  sleep 2
fi
finish() {
  "$engine" rm -f "$container" >/dev/null 2>&1 || true
  [ -z "$xvfb" ] || kill "$xvfb" 2>/dev/null || true
}
trap finish EXIT

# The screen stays outside, so no test tool brings a library in.
start_system() {
  "$engine" run -d --init --name "$container" --ipc=host --security-opt label=disable \
    -e DISPLAY -e LIBGL_ALWAYS_SOFTWARE=1 -e GITHUB_REF_TYPE -e GITHUB_REF_NAME \
    -v /tmp/.X11-unix:/tmp/.X11-unix:ro -v "$package:/package/$name:ro" \
    -v "$terminal:/olc.tar.gz:ro" -v "$root:/source:ro" -v "$shots:/shots" \
    "$1" sleep infinity >/dev/null
}

inside() {
  "$engine" exec "$container" sh -c "$1"
}

window() {
  [[ $(xwininfo -root -tree) == *'"OLC Core"'* ]]
}

# What the app menu runs, read from the entry itself.
open_app() {
  local start
  start=$(inside "sed -n 's/^Exec=//p' /usr/share/applications/olc-core.desktop")
  "$engine" exec -d "$container" sh -c "$start >/tmp/app.log 2>&1 & echo \$! > /tmp/app.pid; wait"
  local waited=0
  until window; do
    sleep 2
    waited=$((waited + 2))
    if [ "$waited" -ge "$wait_seconds" ]; then
      echo "No window after $wait_seconds seconds." >&2
      inside 'tail -n 40 /tmp/app.log' >&2 || true
      return 1
    fi
  done
}

close_app() {
  inside 'kill $(cat /tmp/app.pid)'
  local gone=0
  while window && [ "$gone" -lt 20 ]; do
    sleep 1
    gone=$((gone + 1))
  done
}

shot() {
  import -window root "$shots/$1.png"
}

# Fedora, or Arch Linux: everything linux.sh proves for the .deb, but the Wayland desktop.
full() {
  local image=$1 install=$2 tools=$3 remove=$4
  start_system "$image"
  inside "$install"
  open_app
  sleep 8
  shot first-start
  close_app

  inside "$tools"
  inside 'mkdir -p /tmp/olc && tar -xzf /olc.tar.gz -C /tmp/olc --strip-components=1 &&
    python3 /source/packaging/test/mine.py "$(command -v olc-core)" /tmp/olc/olc /shots'

  # The app's own Start mining, already pressed.
  inside 'mkdir -p "$HOME/.olc" &&
    printf "{\"setup_done\":true,\"mine_on_start\":true,\"hash_cap\":0}" > "$HOME/.olc/core.json"'
  open_app
  sleep 30
  shot mining
  close_app

  inside 'entry=/usr/share/applications/olc-core.desktop
    desktop-file-validate "$entry"
    for size in 48x48 128x128 256x256 512x512; do
      test -f "/usr/share/icons/hicolor/$size/apps/olc-core.png"
    done
    test -f /usr/share/licenses/olc-core/LICENSE
    /usr/lib/olc-core/espflash --version'
  # uaccess hands the board to whoever sits at the computer, with no group to join. It works only
  # before 73-seat-late.rules.
  inside 'rule=/usr/lib/udev/rules.d/70-olc-boards.rules
    udevadm verify "$rule"
    grep -c "TAG+=\"uaccess\"" "$rule"
    first=$(find /usr/lib/udev/rules.d /etc/udev/rules.d -name 73-seat-late.rules | head -n 1)
    test -n "$first"
    [ "$(printf "%s\n" "$(basename "$rule")" "$(basename "$first")" | sort | head -n 1)" = 70-olc-boards.rules ]'

  inside "$remove"
  inside 'test ! -e /usr/bin/olc-core
    test ! -e /usr/lib/udev/rules.d/70-olc-boards.rules
    test -f "$HOME/.olc/node/wallet.json"'
  echo 'Removed the app, and the wallet stayed.' | tee "$shots/removed.txt"
  "$engine" rm -f "$container" >/dev/null
}

# Another system: installed with its own tool and opened, nothing more.
opens() {
  local image=$1 install=$2 picture=$3
  start_system "$image"
  inside "$install"
  open_app
  sleep 8
  shot "$picture"
  inside 'ldd --version | head -n 1'
  close_app
  "$engine" rm -f "$container" >/dev/null
}

case $name in
  *.rpm)
    full registry.fedoraproject.org/fedora:latest \
      "dnf install -y -q --setopt=install_weak_deps=False /package/$name" \
      'dnf install -y -q python3 desktop-file-utils systemd-udev' \
      'dnf remove -y -q olc-core'
    opens registry.opensuse.org/opensuse/tumbleweed:latest \
      "zypper --non-interactive install --allow-unsigned-rpm --no-recommends /package/$name" \
      opensuse-tumbleweed
    opens docker.io/library/almalinux:10 \
      "dnf install -y -q --setopt=install_weak_deps=False /package/$name" \
      almalinux-10
    ;;
  *.pkg.tar.zst)
    full docker.io/library/archlinux:latest \
      "pacman -Syu --noconfirm && pacman -U --noconfirm /package/$name" \
      'pacman -S --noconfirm --needed python desktop-file-utils' \
      'pacman -R --noconfirm olc-core'
    ;;
  *)
    echo "No test for $name" >&2
    exit 1
    ;;
esac
