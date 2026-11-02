#!/usr/bin/env bash
# Builds the .deb from the folder that also becomes the Linux tar.gz.
# Usage: deb.sh <unpacked folder> <deb architecture> <version> <output file>
# Its Depends come from what the programs link against, so the list cannot drift from them, plus
# the libraries the window opens while running, which no linker records (deb-loaded.txt).
set -euo pipefail

source=$1
arch=$2
version=$3
output=$4
here=$(cd "$(dirname "$0")" && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
package=$work/package

# The flashing tool sits beside the app, where the app looks first, and off everyone's search path.
mkdir -p "$package/usr/lib/olc-core" "$package/usr/bin" "$package/usr/share/doc/olc-core" \
  "$package/usr/lib/udev/rules.d" "$package/DEBIAN"
cp "$source/bin/olc-core" "$source/bin/espflash" "$package/usr/lib/olc-core/"
ln -s ../lib/olc-core/olc-core "$package/usr/bin/olc-core"
cp -R "$source/share/." "$package/usr/share/"
cp "$here/70-olc-boards.rules" "$package/usr/lib/udev/rules.d/"
cp "$here/../LICENSE" "$package/usr/share/doc/olc-core/copyright"

# dpkg-shlibdeps reads its package name from a control file in the folder it runs in.
mkdir -p "$work/shlibs/debian"
printf 'Source: olc-core\n\nPackage: olc-core\nArchitecture: any\n' > "$work/shlibs/debian/control"
linked=$(cd "$work/shlibs" && dpkg-shlibdeps -O \
  "$package/usr/lib/olc-core/olc-core" "$package/usr/lib/olc-core/espflash" 2>/dev/null)
linked=${linked#shlibs:Depends=}
loaded=$(paste -sd, "$here/deb-loaded.txt" | sed 's/,/, /g')

size=$(du -sk "$package/usr" | cut -f1)
cat > "$package/DEBIAN/control" <<EOF
Package: olc-core
Version: $version
Architecture: $arch
Maintainer: OfflineCoin <needmoretruth@protonmail.com>
Installed-Size: $size
Depends: $linked, $loaded
Recommends: bluez
Section: net
Priority: optional
Homepage: https://github.com/needmoretruth/OfflineCoin
Description: OLC Core, the OfflineCoin app
 Runs a node, mines, holds a wallet and manages boards, on networks with no
 internet.
EOF
cp "$here/deb-postinst" "$package/DEBIAN/postinst"
cp "$here/deb-postrm" "$package/DEBIAN/postrm"

find "$package" -type d -exec chmod 755 {} +
find "$package/usr" -type f -exec chmod 644 {} +
chmod 755 "$package/usr/lib/olc-core/olc-core" "$package/usr/lib/olc-core/espflash" \
  "$package/DEBIAN/postinst" "$package/DEBIAN/postrm"

# xz: every system this runs on can read it, and the runners' zstd default is not so.
dpkg-deb --root-owner-group -Zxz --build "$package" "$output"
