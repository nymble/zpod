#!/bin/sh
# ZPOD: work around the USB gadget coming up "deaf" after boot.
# Seen on v0.3.0-m2 + macOS host (2026-10-17): g_ether binds, the Mac
# enumerates (CDC-ECM) and receives the Pi's frames, but host->Pi frames
# never arrive (usb0 rx_packets stuck at 1), so the Mac never gets a DHCP
# lease and falls back to 169.254.x.x. A soft disconnect/reconnect of the
# UDC (forcing a clean re-enumeration) fixes it at runtime.
# This runs once per boot: if usb0 has carrier but has received <= RX_MIN
# frames after WAIT seconds, cycle soft_connect (at most TRIES times).
IF=${IF:-usb0}; WAIT=${WAIT:-25}; RX_MIN=${RX_MIN:-2}; TRIES=${TRIES:-2}
UDC=$(ls /sys/class/udc 2>/dev/null | head -n1)
[ -n "$UDC" ] || exit 0
SC=/sys/class/udc/$UDC/soft_connect
rx() { cat /sys/class/net/$IF/statistics/rx_packets 2>/dev/null || echo 0; }
up() { [ "$(cat /sys/class/net/$IF/carrier 2>/dev/null)" = 1 ]; }
while [ $TRIES -gt 0 ]; do
	sleep "$WAIT"
	if ! up; then echo "zpod-usb-kick: $IF no carrier (host not attached)"; exit 0; fi
	r=$(rx)
	if [ "$r" -gt "$RX_MIN" ]; then echo "zpod-usb-kick: $IF ok (rx=$r)"; exit 0; fi
	echo "zpod-usb-kick: $IF has carrier but rx=$r; re-enumerating $UDC"
	echo disconnect > "$SC"; sleep 2; echo connect > "$SC"
	TRIES=$((TRIES - 1))
done
echo "zpod-usb-kick: done (rx=$(rx))"
