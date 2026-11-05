#!/bin/sh
# Simulator screenshots for the USB-host work (docs/USB-HOST-PLAN.md).
# Runs in the zpod-br container (Linux): sh tools/sim/usbhost/run.sh <out-dir>
set -e
here=$(cd "$(dirname "$0")" && pwd)
apps=$(cd "$here/../../.." && pwd)
out=${1:-/tmp/zpod-usbhost-shots}
mkdir -p "$out"
cd "$apps"
cargo build -q -p zpod-mousemusic -p zpod-gps -p zpod-launcher
T=target/debug
export ZPOD_DATA_DIR=$(mktemp -d)
ZPOD_SIM=$out ZPOD_SIM_SCRIPT=$here/mm-gadget.txt ZPOD_USB_ROLE=gadget ZPOD_HID_SIM=none $T/mousemusic 2>/dev/null || true
ZPOD_SIM=$out ZPOD_SIM_SCRIPT=$here/mm-host.txt ZPOD_USB_ROLE=host ZPOD_HID_SIM=$here/mm-hid.txt $T/mousemusic 2>/dev/null || true
[ -x $T/gps ] && sh "$here/gps.sh" "$out" || true
[ -f "$here/launcher.sh" ] && sh "$here/launcher.sh" "$out" || true
ls "$out"
