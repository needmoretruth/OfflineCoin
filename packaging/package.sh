#!/usr/bin/env bash
# Puts one release file into dist/. Usage: package.sh <rust target> <version>
# The program and the flashing tool must already be built for that target.
set -euo pipefail

target=$1
version=$2
here=$(cd "$(dirname "$0")" && pwd)
root=$here/..
built=$root/target/$target/release
dist=$root/dist
stage=$root/target/package/$target
tool=$(command -v espflash)

rm -rf "$stage"
mkdir -p "$dist" "$stage"

linux() {
  local name=OLC-Core-$version-linux-x86_64
  local top=$stage/$name
  mkdir -p "$top/bin" "$top/share/applications"
  cp "$built/olc-core" "$tool" "$top/bin/"
  cp "$here/olc-core.desktop" "$top/share/applications/"
  local size
  for size in 48x48 128x128 256x256 512x512; do
    mkdir -p "$top/share/icons/hicolor/$size/apps"
    cp "$here/icons/$size/olc-core.png" "$top/share/icons/hicolor/$size/apps/"
  done
  tar -C "$stage" -czf "$dist/$name.tar.gz" "$name"
}

windows() {
  local name=OLC-Core-$version-windows-x86_64
  local top=$stage/$name
  mkdir -p "$top"
  cp "$built/olc-core.exe" "$tool" "$top/"
  (cd "$stage" && powershell -NoProfile -Command \
    "Compress-Archive -Path '$name' -DestinationPath '$name.zip'")
  mv "$stage/$name.zip" "$dist/"
}

mac() {
  local arch=$1
  local name=OLC-Core-$version-macos-$arch
  local app="$stage/OLC Core.app"
  mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
  cp "$built/olc-core" "$tool" "$app/Contents/MacOS/"
  sed "s/@VERSION@/$version/g" "$here/Info.plist" > "$app/Contents/Info.plist"
  local set=$stage/olc-core.iconset
  mkdir -p "$set"
  cp "$here/icons/128x128/olc-core.png" "$set/icon_128x128.png"
  cp "$here/icons/256x256/olc-core.png" "$set/icon_256x256.png"
  cp "$here/icons/512x512/olc-core.png" "$set/icon_512x512.png"
  iconutil -c icns -o "$app/Contents/Resources/olc-core.icns" "$set"
  # Ad-hoc: no paid identity, so Gatekeeper asks the person once. Apple silicon needs at least this.
  codesign --force --sign - "$app/Contents/MacOS/espflash"
  codesign --force --sign - "$app"
  ditto -c -k --keepParent "$app" "$dist/$name.zip"
}

case $target in
  x86_64-unknown-linux-gnu) linux ;;
  x86_64-pc-windows-msvc) windows ;;
  aarch64-apple-darwin) mac apple-silicon ;;
  x86_64-apple-darwin) mac intel ;;
  *) echo "No package for $target" >&2; exit 1 ;;
esac
