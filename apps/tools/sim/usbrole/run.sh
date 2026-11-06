#!/bin/sh
# Simulator screenshots for switching the USB role from the GUI
# (docs/USB-HOST.md): Tools > USB, its confirmations, the GPS and Mouse
# Music prompts. Linux (zpod-br container): sh tools/sim/usbrole/run.sh <out>
set -e
here=$(cd "$(dirname "$0")" && pwd)
apps=$(cd "$here/../../.." && pwd)
out=${1:-/tmp/zpod-usbrole-shots}
mkdir -p "$out"
cd "$apps"
cargo build -q -p zpod-mousemusic -p zpod-gps -p zpod-launcher
. tools/sim/env.sh
T=target/debug
export ZPOD_SIM=$out ZPOD_SIM_USB_SWITCH=ok ZPOD_HID_SIM=none
export ZPOD_APPS_DIR=$(mktemp -d) ZPOD_DATA_DIR=$(mktemp -d)
for a in clock music tetris mousemusic gps; do cp -R packages/$a "$ZPOD_APPS_DIR/$a"; done
fix=$apps/crates/zpod-nmea/tests/fixtures/nd105c-cold-start.nmea
ZPOD_USB_ROLE=gadget ZPOD_SIM_IP_WLAN0= ZPOD_SIM_SCRIPT=$here/launcher-nowifi.txt $T/zpod-launcher 2>/dev/null || true
ZPOD_USB_ROLE=gadget ZPOD_SIM_IP_WLAN0=192.168.1.251 ZPOD_SIM_SCRIPT=$here/launcher-wifi.txt $T/zpod-launcher 2>/dev/null || true
ZPOD_USB_ROLE=gadget ZPOD_SIM_IP_WLAN0=192.168.1.251 ZPOD_GPS_SIM=$fix ZPOD_SIM_SCRIPT=$here/gps-wifi.txt $T/gps 2>/dev/null || true
ZPOD_USB_ROLE=gadget ZPOD_SIM_IP_WLAN0= ZPOD_GPS_SIM=$fix ZPOD_SIM_SCRIPT=$here/gps-nowifi.txt $T/gps 2>/dev/null || true
ZPOD_USB_ROLE=gadget ZPOD_SIM_IP_WLAN0=192.168.1.251 ZPOD_SIM_SCRIPT=$here/mm-wifi.txt $T/mousemusic 2>/dev/null || true
ls "$out"
