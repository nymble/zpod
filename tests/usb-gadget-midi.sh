#!/bin/sh
# Copyright (C) 2026 Paul Lambert
# SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
# Offline test of overlay/usr/lib/zpod/zpod-usb-gadget.sh in a fake tree:
# no configfs, no modprobe, nothing bound. Checks that the default (no midi
# line) builds exactly the network function, and that "midi=on" adds the
# MIDI function and its config link. Run: sh tests/usb-gadget-midi.sh
set -eu
S=$(cd "$(dirname "$0")/.." && pwd)/overlay/usr/lib/zpod/zpod-usb-gadget.sh
T=$(mktemp -d)
trap 'rm -rf "$T"' EXIT
fail() { echo "FAIL: $*"; exit 1; }
run() { # $1 = config text, $2 = case dir
	mkdir -p "$T/$2/udc/20980000.usb"
	printf '%s\n' "$1" > "$T/$2/zpod-usb.txt"
	ZPOD_USB_MIDI_CFG="$T/$2/usb-midi.txt" ZPOD_USB_CFG="$T/$2/zpod-usb.txt" ZPOD_GADGET_DIR="$T/$2/g" ZPOD_UDC_DIR="$T/$2/udc" ZPOD_GADGET_DRYRUN=1 sh "$S" start
}
run "mode=ncm" default
G=$T/default/g
[ -d "$G/functions/ncm.usb0" ] || fail "default: no ncm function"
[ -L "$G/configs/c.1/ncm.usb0" ] || fail "default: ncm not linked"
[ ! -e "$G/functions/midi.usb0" ] || fail "default: midi function present"
[ "$(cat "$G/UDC")" = 20980000.usb ] || fail "default: UDC"
[ "$(cat "$G/strings/0x409/product")" = "ZPOD USB Ethernet" ] || fail "default: product"
run "mode=ncm
midi=on" midi
G=$T/midi/g
[ -L "$G/configs/c.1/ncm.usb0" ] || fail "midi: ncm not linked"
[ -L "$G/configs/c.1/midi.usb0" ] || fail "midi: midi not linked"
[ "$(cat "$G/functions/midi.usb0/in_ports")" = 2 ] || fail "midi: in_ports"
[ "$(cat "$G/functions/midi.usb0/out_ports")" = 2 ] || fail "midi: out_ports"
[ "$(cat "$G/strings/0x409/product")" = "ZPOD USB Ethernet + MIDI" ] || fail "midi: product"
run "midi=maybe" bad
[ ! -e "$T/bad/g/functions/midi.usb0" ] || fail "unknown midi value must mean off"
# /data switch (Tools > USB > USB MIDI) wins over the boot file, both ways
mkdir -p "$T/data-on" "$T/data-off"
echo midi=on > "$T/data-on/usb-midi.txt"
run "mode=ncm" data-on
[ -L "$T/data-on/g/configs/c.1/midi.usb0" ] || fail "/data midi=on must add midi"
echo midi=off > "$T/data-off/usb-midi.txt"
run "mode=ncm
midi=on" data-off
[ ! -e "$T/data-off/g/functions/midi.usb0" ] || fail "/data midi=off must win over the boot file"
[ -L "$T/data-off/g/configs/c.1/ncm.usb0" ] || fail "/data midi=off: ncm still linked"
echo "PASS: gadget script offline (default unchanged, midi=on adds midi.usb0 with 2+2 cables, /data switch wins; nothing bound)"
