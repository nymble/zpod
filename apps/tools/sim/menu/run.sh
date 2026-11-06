#!/bin/sh
# Simulator screenshots of the menu tree (docs/APP-MANIFEST.md) and Tools >
# Updates (docs/BOOT-UPDATES.md). Linux (zpod-br container):
#   sh tools/sim/menu/run.sh <out>
set -e
here=$(cd "$(dirname "$0")" && pwd)
apps=$(cd "$here/../../.." && pwd)
out=${1:-/tmp/zpod-menu-shots}
mkdir -p "$out"
cd "$apps"
cargo build -q -p zpod-launcher
. tools/sim/env.sh
T=target/debug
export ZPOD_SIM=$out ZPOD_HID_SIM=none ZPOD_SIM_USB_SWITCH=ok
export ZPOD_DATA_DIR=$(mktemp -d)
A=$(mktemp -d)
for a in clock music tetris mousemusic gps; do cp -R packages/$a "$A/$a"; done
K=$(mktemp)
printf 'boot_load=on\nboot_support=1\nactive=update\nimage=0.10.0 1a2b3c4 2026-11-06\nupdate=0.10.0 5d6e7f8 2026-11-07\nstate=ok\nreason=update 0.10.0 5d6e7f8 running\n' > "$K"
ZPOD_APPS_DIR=$A ZPOD_CORE_SIM=$K ZPOD_USB_ROLE=gadget ZPOD_SIM_SCRIPT=$here/tree.txt $T/zpod-launcher 2>/dev/null || true
# Tools > USB in host mode: the GPS's VID:PID names its app
ZPOD_APPS_DIR=$A ZPOD_CORE_SIM=$K ZPOD_USB_ROLE=host ZPOD_SIM_SCRIPT=$here/usb.txt $T/zpod-launcher 2>/dev/null || true
# after a failed start: the guard fell back to the image core
printf 'boot_load=off\nboot_support=1\nactive=image\nimage=0.10.0 1a2b3c4 2026-11-06\nupdate=0.10.1 9e8d7c6 2026-11-08\nstate=fallback\nreason=the launcher was not up within 60 s\n' > "$K"
ZPOD_APPS_DIR=$A ZPOD_CORE_SIM=$K ZPOD_USB_ROLE=gadget ZPOD_SIM_SCRIPT=$here/updates-fallback.txt $T/zpod-launcher 2>/dev/null || true
# a test3 card: the old manifests (no [menu]) land in the same places
L=$(mktemp -d)
for a in clock music tetris mousemusic gps; do
	cp -R packages/$a "$L/$a"
	# schema 0 as on test3: [app] only (gps: category = "tool")
	awk '/^\[/ && !/^\[app\]/ { exit } !/^schema/ { print }' packages/$a/manifest.toml > "$L/$a/manifest.toml"
	[ $a != gps ] || echo 'category = "tool"' >> "$L/$a/manifest.toml"
done
ZPOD_APPS_DIR=$L ZPOD_CORE_SIM=$K ZPOD_USB_ROLE=gadget ZPOD_SIM_SCRIPT=$here/legacy.txt $T/zpod-launcher 2>/dev/null || true
rm -rf "$A" "$L" "$K"
ls "$out"
