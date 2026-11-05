#!/bin/sh
# USB role switch (docs/USB-HOST.md): boot files and S35usbgadget role
# detection on a fake device tree. Never touches real devices.
set -u
HERE=$(cd "$(dirname "$0")" && pwd)
B=$HERE/../board/zpod
pass=0 fail=0
ok() { if eval "$2"; then pass=$((pass+1)); echo "ok   $1"; else fail=$((fail+1)); echo "FAIL $1"; fi; }
T=$(mktemp -d)
ok "config.txt: include right after the dwc2 line" \
	"grep -v '^#' $B/config.txt | grep -v '^\$' | grep -A1 '^dtoverlay=dwc2,dr_mode=peripheral' | tail -1 | grep -qx 'include zpod-usb-role.txt'"
ok "config.txt: dwc2 is the last overlay" \
	"[ \"\$(grep '^dtoverlay=' $B/config.txt | tail -1)\" = 'dtoverlay=dwc2,dr_mode=peripheral' ]"
ok "zpod-usb-role.txt: gadget by default (no active line)" \
	"! grep -v '^[[:space:]]*#' $B/zpod-usb-role.txt | grep -q '[^[:space:]]'"
ok "zpod-usb-role.txt: host line present, commented" \
	"grep -qx '#dtparam=dr_mode=host' $B/zpod-usb-role.txt"
ok "post-image installs zpod-usb-role.txt" "grep -q 'zpod-usb-role.txt' $B/post-image.sh"
ok "mdev: ttyUSB/ttyACM for dialout" "grep -q 'ttyUSB\[0-9\]\*	root:dialout 660' $B/post-build.sh && grep -q 'ttyACM' $B/post-build.sh"
ok "zpod user in dialout" "grep -q '^zpod .*dialout' $B/users.table"
S35=$B/rootfs-overlay/etc/init.d/S35usbgadget
for m in peripheral host otg; do printf '%s\0' $m > $T/$m; done
ok "S35 role: peripheral -> gadget" "[ \"\$(ZPOD_DT_USB_ROLE=$T/peripheral sh $S35 role)\" = gadget ]"
ok "S35 role: host -> host" "[ \"\$(ZPOD_DT_USB_ROLE=$T/host sh $S35 role)\" = host ]"
ok "S35 role: otg -> otg" "[ \"\$(ZPOD_DT_USB_ROLE=$T/otg sh $S35 role)\" = otg ]"
ok "S35 role: missing -> gadget" "[ \"\$(ZPOD_DT_USB_ROLE=$T/none sh $S35 role)\" = gadget ]"
ok "zpod-usb-role is executable" "[ -x $B/rootfs-overlay/usr/sbin/zpod-usb-role ]"
for k in USB_DWC2_DUAL_ROLE=y USB_HID=m HID_GENERIC=m USB_SERIAL=m USB_SERIAL_PL2303=m USB_ACM=m; do
	ok "linux.fragment: $k" "grep -qx 'CONFIG_$k' $B/linux.fragment"
done
rm -rf "$T"
echo "$pass passed, $fail failed"
[ "$fail" -eq 0 ]
