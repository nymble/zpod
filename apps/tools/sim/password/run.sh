#!/bin/sh
# Simulator screenshots of the Wi-Fi password page (zpod_ui::textentry):
# on-screen characters with the buttons, a USB keyboard plugged in and
# unplugged mid-entry, and the USB host request. Example text only, never a
# real password. Linux (zpod-br container):  sh tools/sim/password/run.sh <out>
set -e
here=$(cd "$(dirname "$0")" && pwd)
apps=$(cd "$here/../../.." && pwd)
out=${1:-/tmp/zpod-password-shots}
mkdir -p "$out"
cd "$apps"
cargo build -q -p zpod-launcher
cargo build -q -p zpod-ui --example padkeys
T=target/debug
pk() { "$T/examples/padkeys" "$1"; }
. tools/sim/env.sh
export ZPOD_SIM=$out ZPOD_SIM_USB_SWITCH=ok
export ZPOD_DATA_DIR=$(mktemp -d)
A=$(mktemp -d)
for a in clock music tetris mousemusic gps; do cp -R packages/$a "$A/$a"; done
S=$(mktemp)
# 1: no keyboard, USB port in gadget mode (the "kbd" cell offers host mode)
{
	cat "$here/nav.txt"
	echo "shot pw-01-grid-empty"
	pk "Kb"
	echo "wait 150"; echo "shot pw-02-typed-masked"
	echo "key up"; for i in 1 2 3 4 5; do echo "key right"; done
	echo "wait 150"; echo "shot pw-03-kbd-cell"
	echo "key square"; echo "key square"; echo "wait 150"; echo "shot pw-04-digits-symbols"
	echo "key square"; echo "wait 150"; echo "shot pw-05-more-symbols"
	echo "key square"; echo "key triangle"; echo "wait 150"; echo "shot pw-06-revealed"
	echo "key triangle"
	for i in 1 2 3 4 5 6; do echo "key right"; done
	echo "key ok"; echo "wait 300"; echo "shot pw-07-usb-host-confirm"
	echo "key cross"; echo "wait 300"; echo "shot pw-08-back-text-kept"
} > "$S"
ZPOD_APPS_DIR=$A ZPOD_HID_SIM=none ZPOD_USB_ROLE=gadget ZPOD_SIM_SCRIPT=$S $T/zpod-launcher 2>/dev/null || true
# 2: host mode; a keyboard is plugged in, types, then is unplugged: the
# grid takes over with the text kept; finish with the buttons and join
H=$(mktemp)
printf 'wait 1200\nadd keyboard USB Keyboard\nwait 1500\ntype horse battery\nwait 1800\nremove keyboard\n' > "$H"
{
	cat "$here/nav.txt"
	echo "wait 2000"; echo "shot pw-09-keyboard-view"
	echo "wait 1500"; echo "shot pw-10-keyboard-typed"
	echo "wait 2500"; echo "shot pw-11-unplugged-grid"
	pk "!7"
	echo "wait 150"; echo "shot pw-12-buttons-after-unplug"
	echo "key up"; echo "key right"; echo "key right"
	echo "wait 150"; echo "shot pw-13-done-cell"
	echo "key ok"; echo "wait 1800"; echo "shot pw-14-joined"
} > "$S"
ZPOD_APPS_DIR=$A ZPOD_HID_SIM=$H ZPOD_USB_ROLE=host ZPOD_SIM_SCRIPT=$S $T/zpod-launcher 2>/dev/null || true
rm -rf "$A" "$S" "$H"
ls "$out"
