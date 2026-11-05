#!/usr/bin/env bash
# Builds the Arch Linux package from the folder that also becomes the Linux tar.gz. Runs as root on
# Arch Linux (distros.sh starts one in a container), and makepkg itself as a plain user.
# Usage: arch.sh <unpacked folder> <version> <output file>
# Its depends are the packages that hold each library the programs link against, and each library
# the window opens while running (loaded-libraries.txt), read from Arch's own list of files.
set -euo pipefail

folder=$(realpath "$1")
# Arch takes no hyphen in a version.
version=${2//-/}
output=$(realpath "$3")
here=$(cd "$(dirname "$0")" && pwd)

pacman -Syu --noconfirm --needed fakeroot binutils
pacman -Fy --noconfirm >/dev/null

owner() {
  local found
  found=$(pacman -Fq "usr/lib/$1" | head -n 1)
  if [ -z "$found" ]; then
    echo "No Arch package holds $1." >&2
    return 1
  fi
  echo "${found#*/}"
}

libraries=$(readelf -d "$folder"/bin/* | sed -n 's/.*Shared library: \[\(.*\)\]/\1/p' \
  | grep -v '^ld-linux'; cat "$here/loaded-libraries.txt")
depends=$(for library in $(echo "$libraries" | sort -u); do owner "$library"; done | sort -u | xargs)
echo "Depends: $depends"

id -u builder >/dev/null 2>&1 || useradd -m builder
work=$(mktemp -d)
cp "$here/arch.install" "$work/"
sed -e "s#@VERSION@#$version#" -e "s#@DEPENDS@#$depends#" -e "s#@FOLDER@#$folder#g" \
  -e "s#@PACKAGING@#$here#g" "$here/PKGBUILD" > "$work/PKGBUILD"
chown -R builder "$work"
# The depends are for the computer it goes on. This one needs none of them to pack it.
su builder -c "cd '$work' && PKGDEST='$work/out' \
  PACKAGER='OfflineCoin <needmoretruth@protonmail.com>' makepkg --nodeps --noconfirm"
cp "$work"/out/olc-core-*.pkg.tar.zst "$output"
rm -rf "$work"
pacman -Qip "$output"
