#!/bin/sh
# Launcher screens for USB host mode: Tools > GPS, Join network password page.
here=$(cd "$(dirname "$0")" && pwd)
apps=$(cd "$here/../../.." && pwd)
out=$1
cd "$apps"
. tools/sim/env.sh
export ZPOD_SIM=$out
export ZPOD_APPS_DIR=$(mktemp -d) ZPOD_DATA_DIR=$(mktemp -d)
for a in clock music tetris mousemusic gps; do cp -R packages/$a "$ZPOD_APPS_DIR/$a"; done
ZPOD_USB_ROLE=host ZPOD_HID_SIM=$here/pw-hid.txt ZPOD_SIM_SCRIPT=$here/launcher-host.txt target/debug/zpod-launcher 2>/tmp/launcher-host.log || true
ZPOD_USB_ROLE=gadget ZPOD_HID_SIM=none ZPOD_SIM_SCRIPT=$here/launcher-gadget.txt target/debug/zpod-launcher 2>/dev/null || true
