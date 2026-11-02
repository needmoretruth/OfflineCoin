Download the file for your computer and open it. Plug in a board and the app finds it. The files named `OLC-Core` are the app. The files named `olc` are the same core for a terminal, with no window.

**Windows.** These builds are not signed. Windows shows "Windows protected your PC". Choose More info, then Run anyway.

**macOS.** These builds are not notarized. macOS says it cannot check the app. Try to open it once, then go to System Settings, Privacy & Security, and choose Open Anyway.

**Linux.** Unpack the file and run `bin/olc-core`. To have it in your app menu, copy `bin` and `share` into `~/.local`. To reach a board your account must be allowed to use serial ports. On most systems that takes `sudo usermod -aG dialout $USER`, then logging in again. Bluetooth needs BlueZ and the D-Bus library. If the app says `libdbus-1.so.3` is missing, install `libdbus-1-3`.
