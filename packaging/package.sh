#!/usr/bin/env bash
# Puts the release files of one system into dist/: an installer, a plain copy of the app for people
# who do not want to install, and the terminal program apart from both. No file name has a version
# in it, so the links in the README keep working from one release to the next.
# Usage: package.sh <rust target> <version>
# Both programs and the flashing tool must already be built for that target.
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
  local arch=$1 deb_arch=$2
  local name=OLC-Core-linux-$arch
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
  "$here/deb.sh" "$top" "$deb_arch" "$version" "$dist/$name.deb"
  "$here/check-deb.sh" "$dist/$name.deb"
}

windows() {
  local name=OLC-Core-windows-x86_64
  local top=$stage/$name
  mkdir -p "$top"
  cp "$built/olc-core.exe" "$tool" "$top/"
  (cd "$stage" && powershell -NoProfile -Command \
    "Compress-Archive -Path '$name' -DestinationPath '$name.zip'")
  mv "$stage/$name.zip" "$dist/"
  OLC_VERSION=$version \
    OLC_SOURCE=$(cygpath -w "$top") \
    OLC_ICON=$(cygpath -w "$here/icons/olc-core.ico") \
    OLC_OUTPUT=$(cygpath -w "$dist") \
    iscc "$(cygpath -w "$here/olc-core.iss")"
}

mac() {
  local arch=$1
  local name=OLC-Core-macos-$arch
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
  disk_image "$app" "$dist/$name.dmg"
}

# The app and a shortcut to Applications side by side, so installing is one drag.
disk_image() {
  local app=$1 image=$2
  local disk=$stage/disk
  mkdir -p "$disk"
  ditto "$app" "$disk/$(basename "$app")"
  ln -s /Applications "$disk/Applications"
  local try
  # hdiutil sometimes finds the runner's disk busy for a moment.
  for try in 1 2 3 4 5; do
    hdiutil create -volname "OLC Core" -srcfolder "$disk" -format UDZO -ov "$image" && return
    sleep 5
  done
  return 1
}

# The terminal program: one file beside the flashing tool, and no window in it.
terminal() {
  local name=olc-$1
  local top=$stage/$name
  mkdir -p "$top"
  if [ "$1" = windows-x86_64 ]; then
    cp "$built/olc.exe" "$tool" "$top/"
    (cd "$stage" && powershell -NoProfile -Command \
      "Compress-Archive -Path '$name' -DestinationPath '$name.zip'")
    mv "$stage/$name.zip" "$dist/"
  else
    cp "$built/olc" "$tool" "$top/"
    tar -C "$stage" -czf "$dist/$name.tar.gz" "$name"
  fi
}

case $target in
  x86_64-unknown-linux-gnu) linux x86_64 amd64; terminal linux-x86_64 ;;
  aarch64-unknown-linux-gnu) linux arm64 arm64; terminal linux-arm64 ;;
  x86_64-pc-windows-msvc) windows; terminal windows-x86_64 ;;
  aarch64-apple-darwin) mac apple-silicon; terminal macos-apple-silicon ;;
  x86_64-apple-darwin) mac intel; terminal macos-intel ;;
  *) echo "No package for $target" >&2; exit 1 ;;
esac
