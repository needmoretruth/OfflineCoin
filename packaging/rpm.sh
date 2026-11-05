#!/usr/bin/env bash
# Builds the .rpm from the folder that also becomes the Linux tar.gz, for this machine's
# architecture. Runs on Fedora with rpm-build and binutils (distros.sh starts one in a container).
# Usage: rpm.sh <unpacked folder> <version> <output file>
set -euo pipefail

folder=$(realpath "$1")
# rpm takes no hyphen in a version, and sorts a tilde before the release it leads to.
version=${2//-/\~}
output=$3
here=$(cd "$(dirname "$0")" && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

rpmbuild -bb --quiet \
  --define "_topdir $work" \
  --define "olc_version $version" \
  --define "olc_folder $folder" \
  --define "olc_packaging $here" \
  "$here/olc-core.spec"
cp "$work"/RPMS/*/olc-core-*.rpm "$output"

# Every library a program links against must be asked for, or the app breaks on someone's computer.
requires=$(rpm -qp --requires "$output")
echo "$requires"
missing=0
for library in $(readelf -d "$folder"/bin/* | sed -n 's/.*Shared library: \[\(.*\)\]/\1/p' \
  | sort -u | grep -v '^ld-linux'); do
  if ! grep -q "^$library(" <<< "$requires"; then
    echo "Not asked for: $library" >&2
    missing=1
  fi
done
exit "$missing"
