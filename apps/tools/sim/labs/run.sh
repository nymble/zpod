#!/bin/sh
# Simulator screenshots for the test7 lab apps (docs/ROADMAP-LABS.md,
# docs/SYNTH.md): USB Lab on a fake sysfs, BLE Lab and Wi-Fi Lab replaying
# the crate sample files, Synth. No radios, no USB, no helper.
# Linux (zpod-br container): sh tools/sim/labs/run.sh <out>
set -e
here=$(cd "$(dirname "$0")" && pwd)
apps=$(cd "$here/../../.." && pwd)
out=${1:-/tmp/zpod-labs-shots}
mkdir -p "$out"
cd "$apps"
cargo build -q -p zpod-usb-lab -p zpod-ble-lab -p zpod-wifi-lab -p zpod-synth
cargo build -q -p zpod-wlan --example rid-test-pcap
. tools/sim/env.sh
T=target/debug
W=$(mktemp -d)
export ZPOD_SIM=$out ZPOD_SIM_USB_SWITCH=ok ZPOD_HID_SIM=none ZPOD_APPDATA=$W/appdata ZPOD_DATA_DIR=$W/data
python3 "$here/fake-usb.py" "$W/usb"
mkdir -p "$W/nonet"
ZPOD_USB_ROLE=host ZPOD_USB_SYSFS=$W/usb ZPOD_SIM_SCRIPT=$here/usb-lab.txt $T/usb-lab 2>"$out/usb-lab.log" || true
ZPOD_USB_ROLE=gadget ZPOD_SIM_SCRIPT=$here/usb-lab-gadget.txt $T/usb-lab 2>>"$out/usb-lab.log" || true
ZPOD_BLE_REPLAY=$apps/crates/zpod-ble/tests/data/ble-replay.hex ZPOD_SIM_SCRIPT=$here/ble-lab.txt $T/ble-lab 2>"$out/ble-lab.log" || true
ZPOD_WLAN_REPLAY=$apps/crates/zpod-wlan/tests/data/rid-sample.pcap ZPOD_SIM_SCRIPT=$here/wifi-lab.txt $T/wifi-lab 2>"$out/wifi-lab.log" || true
# offline TEST aircraft (moving track, encode only) replayed into Wi-Fi Lab
$T/examples/rid-test-pcap "$W/test-track.pcap" 37.7749 -122.4194 12 45 1 2 60 6 >/dev/null
ZPOD_WLAN_REPLAY=$W/test-track.pcap ZPOD_SIM_SCRIPT=$here/wifi-lab-test.txt $T/wifi-lab 2>>"$out/wifi-lab.log" || true
ZPOD_USB_ROLE=host ZPOD_WLAN_SYSFS=$W/nonet ZPOD_SIM_SCRIPT=$here/wifi-lab-none.txt $T/wifi-lab 2>>"$out/wifi-lab.log" || true
ZPOD_SIM_SCRIPT=$here/synth.txt $T/synth 2>"$out/synth.log" || true
ls "$W/appdata"/* 2>/dev/null | sed 's/^/saved: /'
rm -rf "$W"
ls "$out"
