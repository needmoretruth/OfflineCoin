#!/usr/bin/env bash
# Puts the Flatpak for this machine's architecture into a folder: one file that installs on any
# Linux with Flatpak, made from the Linux tar.gz, the same folder the .deb comes from. Installing it
# fetches its runtime from Flathub by itself. Needs flatpak and flatpak-builder.
# Usage: flatpak.sh <OLC-Core-linux-*.tar.gz> <version> <output folder>
set -euo pipefail

tarball=$(realpath "$1")
version=$2
dist=$(realpath "$3")
here=$(cd "$(dirname "$0")" && pwd)
app=io.github.needmoretruth.OfflineCoin
flathub=https://dl.flathub.org/repo/flathub.flatpakrepo
arch=$(flatpak --default-arch)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

mkdir "$work/folder"
tar -xzf "$tarball" -C "$work/folder" --strip-components=1
cp "$here/flatpak/$app.yml" "$here/../LICENSE" "$work/"
sed -e "s/@VERSION@/$version/" -e "s/@DATE@/$(date -u +%F)/" "$here/flatpak/$app.metainfo.xml" \
  > "$work/$app.metainfo.xml"

flatpak remote-add --user --if-not-exists flathub "$flathub"
cd "$work"
flatpak-builder --user --install-deps-from=flathub --disable-rofiles-fuse --force-clean \
  --repo=repo build "$app.yml"
flatpak build-bundle --runtime-repo="$flathub" repo "$dist/OLC-Core-linux-$arch.flatpak" "$app"
