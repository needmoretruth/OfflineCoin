#!/usr/bin/env bash
# OLC Core keeps a board as it is when the board already runs the build id the app carries, so a release
# whose firmware changed since the last release must carry a new id in firmware/build-id, or boards never
# get it. "Changed" means a change under firmware/ or under a crate the firmware uses.
# usage: check-build-id.sh [EARLIER [LATER]]   (defaults: the release tag before LATER, and HEAD)
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"
later=${2:-HEAD}
earlier=${1:-$(git describe --tags --abbrev=0 --match 'v*' "$later^" 2>/dev/null || true)}
if [ -z "$earlier" ]; then
  echo "No earlier release to compare with."
  exit 0
fi
paths=(firmware)
while read -r crate; do
  paths+=("${crate#../}")
done < <(git show "$later:firmware/Cargo.toml" | sed -n 's/.*path *= *"\(\.\.\/crates\/[^"]*\)".*/\1/p')
if git diff --quiet "$earlier" "$later" -- "${paths[@]}"; then
  echo "The firmware is the same as in $earlier."
  exit 0
fi
before=$(git show "$earlier:firmware/build-id" | tr -d '[:space:]')
after=$(git show "$later:firmware/build-id" | tr -d '[:space:]')
if [ "$before" = "$after" ]; then
  echo "The firmware changed since $earlier, but firmware/build-id is still $after. Raise it so boards update." >&2
  git diff --stat "$earlier" "$later" -- "${paths[@]}" | tail -1 >&2
  exit 1
fi
echo "The firmware changed since $earlier and its build id went from $before to $after."
