#!/bin/sh
# Copyright (C) 2026 Paul Lambert
# SPDX-License-Identifier: GPL-3.0-or-later OR LicenseRef-Commercial
# ZPOD USB gadget Ethernet (M4): configfs/libcomposite, ONE network function.
#
# Why not g_ether (M2/M3): g_ether exposes two configurations (RNDIS +
# CDC-ECM). macOS binds its DriverKit AppleUserECM driver to the ECM one, and
# that driver stops transmitting after ~161 frames per enumeration (Apple
# FB24614121, raspberrypi/rpi-usb-gadget#27). Host->Pi traffic dies, so the
# Mac never completes DHCP with the Pi and self-assigns 169.254.x.x. The M3
# zpod-usb-kick re-enumeration only resets that budget.
# CDC-NCM uses a different macOS driver (AppleUSBNCM) without that bug, and
# Linux/Windows 11 bind NCM natively too.
#
# Mode: /boot/firmware/zpod-usb.txt, line "mode=ncm" (default), "mode=ecm",
# or "mode=g_ether" (the M3 behaviour). Edit it from the Mac if needed.
# Fixed MACs so the Mac sees the same interface every boot:
#   Mac side 02:5a:50:4f:44:01, Pi side (usb0) 02:5a:50:4f:44:02.
# NetworkManager's "USB Gadget (shared)" profile then serves DHCP on
# 10.12.194.0/28 (Pi = .1) with no default route/DNS for the Mac.
#
# Optional USB MIDI (off by default): "midi=on" adds a class-compliant MIDI
# function (usb_f_midi, 2 in + 2 out cables: cable 1 = notes, cable 2 =
# spare / clock) next to the network function, so the computer sees a ZPOD
# MIDI device on the same cable (Pipit sends to it, Synth plays from it).
# Set from the device with Tools > USB > USB MIDI (zpod-usb-role midi
# on|off), which writes /data/zpod/usb-midi.txt; that file wins over a
# "midi=" line in the boot file. Takes effect when the gadget is (re)built:
# boot, the Tools switch (restarts the link), or a USB role switch. Not
# used with mode=g_ether. docs/SYNTH.md.
#
# Test hooks (offline only, tests/usb-gadget-midi.sh): ZPOD_USB_CFG,
# ZPOD_GADGET_DIR, ZPOD_UDC_DIR, ZPOD_GADGET_DRYRUN=1 (no modprobe/mount),
# ZPOD_USB_MIDI_CFG.
set -eu
CFG=${ZPOD_USB_CFG:-/boot/firmware/zpod-usb.txt}
MIDI_CFG=${ZPOD_USB_MIDI_CFG:-/data/zpod/usb-midi.txt}
HID_CFG=${ZPOD_USB_HID_CFG:-/data/zpod/usb-hid.txt}
MIDI_PORTS=2
DRY=${ZPOD_GADGET_DRYRUN:-}
MODE=ncm
MIDI=off
HID=off
if [ -r "$CFG" ]; then
	m=$(sed -n 's/^[[:space:]]*mode[[:space:]]*=[[:space:]]*\([a-z_]*\).*/\1/p' "$CFG" | tail -n1)
	case "$m" in ncm|ecm|g_ether) MODE=$m ;; "") ;; *) echo "zpod-usb-gadget: unknown mode '$m', using ncm" ;; esac
	v=$(sed -n 's/^[[:space:]]*midi[[:space:]]*=[[:space:]]*\([a-z]*\).*/\1/p' "$CFG" | tail -n1)
	case "$v" in on) MIDI=on ;; off|"") ;; *) echo "zpod-usb-gadget: unknown midi '$v', MIDI off" ;; esac
	v=$(sed -n 's/^[[:space:]]*hid[[:space:]]*=[[:space:]]*\([a-z]*\).*/\1/p' "$CFG" | tail -n1)
	case "$v" in on) HID=on ;; off|"") ;; *) echo "zpod-usb-gadget: unknown hid '$v', HID off" ;; esac
fi
if [ -r "$MIDI_CFG" ]; then
	v=$(sed -n 's/^[[:space:]]*midi[[:space:]]*=[[:space:]]*\([a-z]*\).*/\1/p' "$MIDI_CFG" | tail -n1)
	case "$v" in on) MIDI=on ;; off) MIDI=off ;; *) ;; esac
fi
if [ -r "$HID_CFG" ]; then
	v=$(sed -n 's/^[[:space:]]*hid[[:space:]]*=[[:space:]]*\([a-z]*\).*/\1/p' "$HID_CFG" | tail -n1)
	case "$v" in on) HID=on ;; off) HID=off ;; *) ;; esac
fi
HOST_MAC=02:5a:50:4f:44:01
DEV_MAC=02:5a:50:4f:44:02
G=${ZPOD_GADGET_DIR:-/sys/kernel/config/usb_gadget/zpod}
UDC_DIR=${ZPOD_UDC_DIR:-/sys/class/udc}

udc() { ls "$UDC_DIR" 2>/dev/null | head -n1; }
i=0
while [ -z "$(udc)" ] && [ $i -lt 50 ]; do sleep 0.2; i=$((i + 1)); done
UDC=$(udc)
[ -n "$UDC" ] || { echo "zpod-usb-gadget: no UDC (is dtoverlay=dwc2,dr_mode=peripheral set?)"; exit 0; }

stop() {
	[ -d "$G" ] || return 0
	echo "" > "$G/UDC" 2>/dev/null || true
	for l in "$G"/configs/c.1/*.usb0; do [ -L "$l" ] && rm -f "$l"; done  # ncm/ecm, midi, hid
	rmdir "$G"/configs/c.1/strings/0x409 "$G"/configs/c.1 2>/dev/null || true
	for f in "$G"/functions/*; do [ -d "$f" ] && rmdir "$f"; done
	rmdir "$G"/strings/0x409 "$G" 2>/dev/null || true
}
[ "${1:-start}" = stop ] && { stop; exit 0; }

if [ "$MODE" = g_ether ]; then
	modprobe g_ether host_addr=$HOST_MAC dev_addr=$DEV_MAC iSerialNumber=zpod
	echo "zpod-usb-gadget: g_ether (legacy RNDIS+ECM) on $UDC"
	exit 0
fi

if [ -z "$DRY" ]; then
	modprobe libcomposite
	mountpoint -q /sys/kernel/config || mount -t configfs none /sys/kernel/config
	lsmod | grep -q '^g_ether' && rmmod g_ether || true
	[ "$MIDI" = on ] && { modprobe -q usb_f_midi || MIDI=off; }
	[ "$HID" = on ] && { modprobe -q usb_f_hid || HID=off; }
fi
stop
mkdir -p "$G"
cd "$G"
echo 0x1d6b > idVendor          # Linux Foundation
echo 0x0104 > idProduct         # Multifunction Composite Gadget
echo 0x0100 > bcdDevice
echo 0x0200 > bcdUSB
mkdir -p strings/0x409
echo zpod > strings/0x409/serialnumber
echo ZPOD > strings/0x409/manufacturer
prod="ZPOD USB Ethernet"
[ "$MIDI" = on ] && prod="$prod + MIDI"
[ "$HID" = on ] && prod="$prod + HID"
echo "$prod" > strings/0x409/product
mkdir -p configs/c.1/strings/0x409
echo "$MODE" > configs/c.1/strings/0x409/configuration
echo 250 > configs/c.1/MaxPower
mkdir -p "functions/$MODE.usb0"
echo $HOST_MAC > "functions/$MODE.usb0/host_addr"
echo $DEV_MAC > "functions/$MODE.usb0/dev_addr"
ln -s "functions/$MODE.usb0" configs/c.1/
if [ "$MIDI" = on ]; then
	mkdir -p functions/midi.usb0
	echo ZPOD > functions/midi.usb0/id
	echo $MIDI_PORTS > functions/midi.usb0/in_ports
	echo $MIDI_PORTS > functions/midi.usb0/out_ports
	ln -s functions/midi.usb0 configs/c.1/
fi
if [ "$HID" = on ]; then
	# Boot keyboard (8-byte reports). Descriptor is the standard HID boot keyboard.
	mkdir -p functions/hid.usb0
	echo 1 > functions/hid.usb0/protocol
	echo 1 > functions/hid.usb0/subclass
	echo 8 > functions/hid.usb0/report_length
	# report_desc: Generic Desktop / Keyboard, 8-byte boot report
	printf '\x05\x01\x09\x06\xa1\x01\x05\x07\x19\xe0\x29\xe7\x15\x00\x25\x01\x75\x01\x95\x08\x81\x02\x95\x01\x75\x08\x81\x03\x95\x05\x75\x01\x05\x08\x19\x01\x29\x05\x91\x02\x95\x01\x75\x03\x91\x03\x95\x06\x75\x08\x15\x00\x25\x65\x05\x07\x19\x00\x29\x65\x81\x00\xc0' > functions/hid.usb0/report_desc
	ln -s functions/hid.usb0 configs/c.1/
fi
extra=""
[ "$MIDI" = on ] && extra="$extra + midi"
[ "$HID" = on ] && extra="$extra + hid"
echo "$UDC" > UDC
echo "zpod-usb-gadget: $MODE$extra on $UDC, ifname $(cat functions/$MODE.usb0/ifname 2>/dev/null || echo '?')"
