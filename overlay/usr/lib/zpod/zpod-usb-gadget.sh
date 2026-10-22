#!/bin/sh
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
set -eu
CFG=/boot/firmware/zpod-usb.txt
MODE=ncm
if [ -r "$CFG" ]; then
	m=$(sed -n 's/^[[:space:]]*mode[[:space:]]*=[[:space:]]*\([a-z_]*\).*/\1/p' "$CFG" | tail -n1)
	case "$m" in ncm|ecm|g_ether) MODE=$m ;; "") ;; *) echo "zpod-usb-gadget: unknown mode '$m', using ncm" ;; esac
fi
HOST_MAC=02:5a:50:4f:44:01
DEV_MAC=02:5a:50:4f:44:02
G=/sys/kernel/config/usb_gadget/zpod

udc() { ls /sys/class/udc 2>/dev/null | head -n1; }
i=0
while [ -z "$(udc)" ] && [ $i -lt 50 ]; do sleep 0.2; i=$((i + 1)); done
UDC=$(udc)
[ -n "$UDC" ] || { echo "zpod-usb-gadget: no UDC (is dtoverlay=dwc2,dr_mode=peripheral set?)"; exit 0; }

stop() {
	[ -d "$G" ] || return 0
	echo "" > "$G/UDC" 2>/dev/null || true
	for l in "$G"/configs/c.1/*.usb0; do [ -L "$l" ] && rm -f "$l"; done
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

modprobe libcomposite
mountpoint -q /sys/kernel/config || mount -t configfs none /sys/kernel/config
lsmod | grep -q '^g_ether' && rmmod g_ether || true
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
echo "ZPOD USB Ethernet" > strings/0x409/product
mkdir -p configs/c.1/strings/0x409
echo "$MODE" > configs/c.1/strings/0x409/configuration
echo 250 > configs/c.1/MaxPower
mkdir -p "functions/$MODE.usb0"
echo $HOST_MAC > "functions/$MODE.usb0/host_addr"
echo $DEV_MAC > "functions/$MODE.usb0/dev_addr"
ln -s "functions/$MODE.usb0" configs/c.1/
echo "$UDC" > UDC
echo "zpod-usb-gadget: $MODE on $UDC, ifname $(cat functions/$MODE.usb0/ifname 2>/dev/null || echo '?')"
