# The .rpm for Fedora, openSUSE and the RHEL family, built by rpm.sh from the folder that is also the
# Linux tar.gz. rpm reads Requires for every library the programs link against by itself. The window
# opens more while running, which no linker records, so those come from loaded-libraries.txt as the
# library names rpm already knows, and each system finds the package that holds them.

# The programs are built and stripped already, and carry no debug files.
%global debug_package %{nil}
%global __strip /bin/true
%global _build_id_links none
# xz and the version 4 format: every rpm in use reads them, and Fedora's newer defaults are not so.
%global _binary_payload w6.xzdio
%global _rpmformat 4

Name:           olc-core
Version:        %{olc_version}
Release:        1
Summary:        OLC Core, the OfflineCoin app
License:        Apache-2.0
URL:            https://github.com/needmoretruth/OfflineCoin
%{lua:
for name in io.lines(rpm.expand("%{olc_packaging}/loaded-libraries.txt")) do
  print("Requires: " .. name .. "()(64bit)\n")
end
}
Recommends:     bluez

%description
Runs a node, mines, holds a wallet and manages boards, on networks with no
internet.

%install
# The flashing tool sits beside the app, where the app looks first, and off everyone's search path.
mkdir -p %{buildroot}/usr/lib/olc-core %{buildroot}/usr/bin %{buildroot}/usr/share \
  %{buildroot}/usr/lib/udev/rules.d %{buildroot}/usr/share/licenses/olc-core
install -m 755 %{olc_folder}/bin/olc-core %{olc_folder}/bin/espflash %{buildroot}/usr/lib/olc-core/
ln -s ../lib/olc-core/olc-core %{buildroot}/usr/bin/olc-core
cp -R %{olc_folder}/share/. %{buildroot}/usr/share/
install -m 644 %{olc_packaging}/70-olc-boards.rules %{buildroot}/usr/lib/udev/rules.d/
install -m 644 %{olc_packaging}/../LICENSE %{buildroot}/usr/share/licenses/olc-core/

%post
# A board already plugged in follows the new rule without being plugged in again.
if [ -e /run/udev/control ]; then
  udevadm control --reload-rules || :
  udevadm trigger --subsystem-match=tty --action=change || :
fi

%postun
# Wallets and chains stay in the person's home folder. Only the rule goes.
if [ "$1" -eq 0 ] && [ -e /run/udev/control ]; then
  udevadm control --reload-rules || :
fi

%files
/usr/bin/olc-core
/usr/lib/olc-core
/usr/lib/udev/rules.d/70-olc-boards.rules
/usr/share/applications/olc-core.desktop
/usr/share/icons/hicolor/*/apps/olc-core.png
%dir /usr/share/licenses/olc-core
%license /usr/share/licenses/olc-core/LICENSE
