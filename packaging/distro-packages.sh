#!/usr/bin/env bash
# Puts the .rpm, and on x86_64 the Arch Linux package, into a folder. Each is made from the Linux
# tar.gz, the same folder the .deb comes from, by its own system's tools in a container of that
# system. Usage: distro-packages.sh <OLC-Core-linux-*.tar.gz> <version> <output folder>
set -euo pipefail

tarball=$(realpath "$1")
version=$2
dist=$(realpath "$3")
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/.." && pwd)
engine=${CONTAINER:-docker}
arch=$(uname -m)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

tar -xzf "$tarball" -C "$work"
folder=$(find "$work" -mindepth 1 -maxdepth 1 -type d)
# The finished file goes to whoever runs this. Rootless podman already maps its root to them.
owner="$(id -u):$(id -g)"
if [ "$engine" = podman ] && [ "$(id -u)" != 0 ]; then
  owner=0:0
fi

# The repository and the folder go in read-only. Only the finished file comes back out.
in_container() {
  local image=$1 script=$2
  "$engine" run --rm --security-opt label=disable -e "OWNER=$owner" \
    -v "$root:/source:ro" -v "$folder:/folder:ro" -v "$dist:/dist" "$image" sh -c "$script"
}

in_container registry.fedoraproject.org/fedora:latest "
  dnf install -y -q rpm-build binutils &&
  /source/packaging/rpm.sh /folder '$version' /dist/OLC-Core-linux-$arch.rpm &&
  chown \$OWNER /dist/OLC-Core-linux-$arch.rpm"

# Arch Linux is made for x86_64 alone.
if [ "$arch" = x86_64 ]; then
  in_container docker.io/library/archlinux:latest "
    /source/packaging/arch.sh /folder '$version' /dist/OLC-Core-linux-x86_64.pkg.tar.zst &&
    chown \$OWNER /dist/OLC-Core-linux-x86_64.pkg.tar.zst"
fi
