#!/usr/bin/env bash
# Fails when a file the READMEs link to is missing from a release, so a link can never lead nowhere.
# Usage: check-names.sh <folder of release files>
set -euo pipefail

dist=$1
here=$(cd "$(dirname "$0")" && pwd)
root=$here/..
installers=$(grep -oh 'releases/latest/download/[^)]*' "$root/README.md" "$root/README.ko.md" \
  | sed 's#.*/##' | sort -u)
others="OLC-Core-windows-x86_64.zip OLC-Core-macos-apple-silicon.zip OLC-Core-macos-intel.zip
OLC-Core-linux-x86_64.tar.gz OLC-Core-linux-arm64.tar.gz olc-windows-x86_64.zip
olc-macos-apple-silicon.tar.gz olc-macos-intel.tar.gz olc-linux-x86_64.tar.gz olc-linux-arm64.tar.gz"

count=$(echo "$installers" | wc -l)
if [ "$count" -ne 5 ]; then
  echo "The READMEs link to $count files, and 5 were expected." >&2
  exit 1
fi
absent=0
for name in $installers $others; do
  if [ -f "$dist/$name" ]; then
    echo "found $name"
  else
    echo "missing $name" >&2
    absent=$((absent + 1))
  fi
done
exit "$absent"
