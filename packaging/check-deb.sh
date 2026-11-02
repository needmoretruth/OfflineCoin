#!/usr/bin/env bash
# Compares a .deb's Depends with what its programs really link against on this architecture.
# A library missing from the list breaks the app on someone's computer. A package listed for no
# reason gets in the way of installing. Usage: check-deb.sh <file.deb>
set -euo pipefail

deb=$1
here=$(cd "$(dirname "$0")" && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
triplet=$(dpkg-architecture -qDEB_HOST_MULTIARCH)
dpkg-deb -x "$deb" "$work"

depends=$(dpkg-deb -f "$deb" Depends | tr ',' '\n' | sed 's/^ *//; s/ .*//' | sort -u)
loaded=$(sort -u "$here/deb-loaded.txt")

# The dynamic loader belongs to libc6, and its path differs between systems.
linked=$(for program in "$work"/usr/lib/olc-core/*; do
  readelf -d "$program" | sed -n 's/.*Shared library: \[\(.*\)\]/\1/p'
done | sort -u | grep -v '^ld-linux' | while read -r library; do
  dpkg -S "*/$triplet/$library" | sed -n 's/:.*//p' | head -n 1
done | sort -u)

echo "Linked: $(echo "$linked" | xargs)"
echo "Opened while running: $(echo "$loaded" | xargs)"
echo "Listed: $(echo "$depends" | xargs)"

missing=$(comm -23 <(echo "$linked") <(echo "$depends"))
extra=$(comm -13 <(sort -u <(echo "$linked"; echo "$loaded")) <(echo "$depends"))
if [ -n "$missing" ] || [ -n "$extra" ]; then
  echo "Not listed: ${missing:-none}. Listed for no reason: ${extra:-none}." >&2
  exit 1
fi
