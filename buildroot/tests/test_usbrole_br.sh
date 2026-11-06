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
for k in USB_DWC2_DUAL_ROLE=y OF_OVERLAY=y OF_CONFIGFS=y USB_HID=m HID_GENERIC=m USB_SERIAL=m USB_SERIAL_PL2303=m USB_ACM=m; do
	ok "linux.fragment: $k" "grep -qx 'CONFIG_$k' $B/linux.fragment"
done
# older /data/etc/group (test3): dialout without zpod; a user-added member stays
GS=$B/rootfs-overlay/usr/lib/zpod/zpod-group-sync
printf 'root:x:0:\naudio:x:29:zpod\ndialout:x:18:zpod\ninput:x:103:zpod\n' > $T/base
printf 'root:x:0:\naudio:x:29:zpod,paul\ndialout:x:18:\ninput:x:103:zpod\n' > $T/data
sh $GS $T/base $T/data
ok "group sync: zpod added to dialout" "grep -qx 'dialout:x:18:zpod' $T/data"
ok "group sync: other members kept" "grep -qx 'audio:x:29:zpod,paul' $T/data"
sh $GS $T/base $T/data
ok "group sync: idempotent" "[ \$(grep -c zpod $T/data) = 3 ] && grep -qx 'input:x:103:zpod' $T/data"
ok "zpod-data-early runs the group sync" "grep -q 'zpod-group-sync' $B/rootfs-overlay/usr/sbin/zpod-data-early"

# ---- zpod-usb-role: live switch (dry run on a fake sysfs), next boot, guard ----
UR=$B/rootfs-overlay/usr/sbin/zpod-usb-role
DTBO=$B/rootfs-overlay/usr/lib/zpod/usb-role
for m in host peripheral otg; do
	ok "usb-role/$m.dtbo is a dtb" "[ \"\$(od -An -tx1 -N4 $DTBO/$m.dtbo | tr -d ' ')\" = d00dfeed ]"
	ok "usb-role/$m.dts sets dr_mode=$m on usb@7e980000" "grep -q 'target-path = \"/soc/usb@7e980000\"' $DTBO/$m.dts && grep -q 'dr_mode = \"$m\"' $DTBO/$m.dts"
done
if command -v dtc >/dev/null 2>&1; then
	ok "host.dtbo decompiles to dr_mode host" "dtc -I dtb -O dts $DTBO/host.dtbo 2>/dev/null | grep -q 'dr_mode = \"host\"'"
fi
R=$T/ur
mk() { # $1 = dr_mode now; fresh fake device, test3 config.txt form
	rm -rf "$R"; mkdir -p "$R/sys/bus/platform/drivers/dwc2" "$R/sys/kernel/config/device-tree/overlays" "$R/sys/class/udc/20980000.usb" "$R/run" "$R/state" "$R/boot"
	printf '%s\0' "$1" > "$R/dr_mode"
	printf 'dtoverlay=vc4-kms-v3d\ndtoverlay=dwc2,dr_mode=peripheral\n' > "$R/boot/config.txt"
}
ur() { ZPOD_UR_S35=${URS35:-$S35} ZPOD_UR_SYS=$R/sys ZPOD_UR_DT=$R/dr_mode ZPOD_UR_RUN=$R/run ZPOD_UR_STATE=$R/state ZPOD_UR_DRY=1 ZPOD_UR_DTBO=$DTBO ZPOD_UR_BOOT=$R/boot ZPOD_UR_GUARD_SECS=1 ZPOD_UR_LAUNCHER_SOCK=$R/run/launcher.sock sh "$UR" "$@"; }
nowifi() { ZPOD_UR_WIFI_IP= ZPOD_UR_SSH=0 ur "$@"; }
wifi() { ZPOD_UR_WIFI_IP=192.168.1.251 ZPOD_UR_SSH=1 ur "$@"; }
mk peripheral
nowifi status > $T/st
ok "status: gadget, not live, next gadget (config.txt form)" "grep -qx role=gadget $T/st && grep -qx live=0 $T/st && grep -qx next=gadget $T/st && grep -qx persist=config $T/st"
ok "status: can_switch, link from the UDC, not safe without Wi-Fi" "grep -qx can_switch=1 $T/st && grep -qx safe=0 $T/st && grep -q '^link=' $T/st"
ok "check: no Wi-Fi -> exit 3" "nowifi check >/dev/null; [ \$? = 3 ]"
ok "check: Wi-Fi but no sshd -> exit 3" "ZPOD_UR_WIFI_IP=10.0.0.2 ZPOD_UR_SSH=0 ur check | grep -q 'SSH is not listening'"
ok "check: Wi-Fi + sshd -> exit 0" "wifi check | grep -qx 'ok Wi-Fi SSH root@192.168.1.251'"
nowifi set host > $T/o; rc=$?
ok "set host without Wi-Fi SSH: refused (exit 3), nothing done" "[ $rc = 3 ] && grep -q '^err no Wi-Fi SSH' $T/o && ! grep -q '^do:' $T/o && [ ! -d $R/sys/kernel/config/device-tree/overlays/zpod-usb-role ]"
nowifi set host --force > $T/o
ok "set host --force: stop gadget, unbind, overlay, bind (in order)" "[ \"\$(grep '^do:' $T/o | tr '\n' ,)\" = 'do: gadget stop,do: unbind,do: overlay host,do: bind,' ] && tail -1 $T/o | grep -qx 'ok host'"
nowifi status > $T/st
ok "after host: role host, live, boot gadget, run/usb-role" "grep -qx role=host $T/st && grep -qx live=1 $T/st && grep -qx boot=gadget $T/st && grep -qx host $R/run/usb-role"
ok "set host again: already" "nowifi set host | grep -qx 'ok host (already)'"
nowifi set gadget > $T/o
ok "set gadget (no Wi-Fi needed): unbind, overlay off, bind, gadget start" "[ \"\$(grep '^do:' $T/o | tr '\n' ,)\" = 'do: unbind,do: overlay off,do: bind,do: gadget start,' ] && tail -1 $T/o | grep -qx 'ok gadget'"
ok "after gadget: no overlay, not live, lock released" "nowifi status | grep -qx live=0 && [ ! -d $R/sys/kernel/config/device-tree/overlays/zpod-usb-role ] && [ ! -d $R/run/usb-role.lock ]"
ok "set host with Wi-Fi SSH: no --force needed" "wifi set host | tail -1 | grep -qx 'ok host'"
ok "host -> auto: overlay otg" "wifi set auto | grep -q 'do: overlay otg' && nowifi status | grep -qx role=otg"
nowifi set gadget >/dev/null
ok "no lock left after a switch" "[ ! -e $R/run/usb-role.lock ]"
mkdir $R/run/usb-role.lock; echo $$ > $R/run/usb-role.lock/pid
ok "second switch while one runs: exit 5" "wifi set host >/dev/null; [ \$? = 5 ]"
sh -c 'exit 0' & dead=$!; wait $dead; echo $dead > $R/run/usb-role.lock/pid
ok "stale lock (its pid is gone) is taken over" "wifi set host | tail -1 | grep -qx 'ok host' && [ ! -e $R/run/usb-role.lock ]"
nowifi set gadget >/dev/null
ok "set: bad target / option -> exit 2" "nowifi set sideways >/dev/null; [ \$? = 2 ] && { nowifi set host --now >/dev/null; [ \$? = 2 ]; }"
ok "usb.log has no Wi-Fi names or passwords" "! grep -qiE 'ssid|psk|pass' $R/run/usb.log"
# next boot, test3 form (the one dtoverlay=dwc2 line in config.txt)
cp $R/boot/config.txt $T/config.orig
ok "next-boot host (config.txt form)" "nowifi next-boot host | grep -qx 'ok next boot host (config.txt)' && grep -qx 'dtoverlay=dwc2,dr_mode=host' $R/boot/config.txt"
ok "next-boot: backup config.txt.zpod-prev, no temp left" "cmp -s $T/config.orig $R/boot/config.txt.zpod-prev && [ ! -e $R/boot/config.txt.zpod-tmp ]"
ok "status: next host" "nowifi status | grep -qx next=host"
ok "status: no boot guard in S35 (test3) -> persist=none" "printf '#!/bin/sh\n' > $T/S35old && URS35=$T/S35old nowifi status | grep -qx persist=none"
ok "next-boot gadget restores config.txt exactly" "nowifi next-boot gadget >/dev/null && cmp -s $T/config.orig $R/boot/config.txt"
ok "next-boot auto: refused (exit 2)" "nowifi next-boot auto >/dev/null; [ \$? = 2 ]"
printf 'dtoverlay=dwc2,dr_mode=peripheral\ndtoverlay=dwc2,dr_mode=host\n' > $R/boot/config.txt
ok "next-boot: two dwc2 lines -> refused, file untouched" "! nowifi next-boot host >/dev/null && [ \$(grep -c dwc2 $R/boot/config.txt) = 2 ] && grep -qx 'dtoverlay=dwc2,dr_mode=peripheral' $R/boot/config.txt"
# test4 form (zpod-usb-role.txt beside config.txt)
cp $B/config.txt $B/zpod-usb-role.txt $R/boot/
ok "status: test4 file form, next gadget" "nowifi status | grep -qx persist=file && nowifi status | grep -qx next=gadget"
ok "next-boot host (zpod-usb-role.txt)" "nowifi next-boot host >/dev/null && grep -qx 'dtparam=dr_mode=host' $R/boot/zpod-usb-role.txt && cmp -s $B/config.txt $R/boot/config.txt"
ok "next-boot gadget restores zpod-usb-role.txt exactly" "nowifi next-boot gadget >/dev/null && cmp -s $B/zpod-usb-role.txt $R/boot/zpod-usb-role.txt"
printf '# empty\n' > $R/boot/zpod-usb-role.txt
ok "next-boot host adds the line to an emptied zpod-usb-role.txt" "nowifi next-boot host >/dev/null && grep -qx 'dtparam=dr_mode=host' $R/boot/zpod-usb-role.txt && nowifi next-boot gadget >/dev/null && grep -qx '#dtparam=dr_mode=host' $R/boot/zpod-usb-role.txt"
cp $B/zpod-usb-role.txt $R/boot/
ok "set host --persist: switched now and next boot host" "wifi set host --persist | tail -1 | grep -qx 'ok host' && nowifi status | grep -qx next=host"
# boot guard: host-mode boot (dr_mode host from the boot partition)
mk host; cp $B/config.txt $B/zpod-usb-role.txt $R/boot/; nowifi next-boot host >/dev/null
ok "boot-guard: launcher never up -> next boot gadget and gadget now" "nowifi boot-guard >/dev/null; nowifi status | grep -qx next=gadget && nowifi status | grep -qx role=gadget"
mk host; cp $B/config.txt $B/zpod-usb-role.txt $R/boot/; nowifi next-boot host >/dev/null; echo 2 > $R/state/host-boots
ok "boot-guard: third failed host boot -> revert at once" "nowifi boot-guard | grep -q 'do: overlay peripheral' ; nowifi status | grep -qx next=gadget"
if command -v python3 >/dev/null 2>&1; then
	mk host; cp $B/config.txt $B/zpod-usb-role.txt $R/boot/; nowifi next-boot host >/dev/null; echo 1 > $R/state/host-boots
	python3 -c "import socket,sys; s=socket.socket(socket.AF_UNIX); s.bind(sys.argv[1])" $R/run/launcher.sock
	ok "boot-guard: launcher up -> stays host, counter cleared" "nowifi boot-guard >/dev/null; nowifi status | grep -qx role=host && nowifi status | grep -qx next=host && [ ! -e $R/state/host-boots ]"
fi
mk peripheral; echo 2 > $R/state/host-boots
ok "boot-guard on a gadget boot: nothing, counter cleared" "[ -z \"\$(nowifi boot-guard)\" ] && [ ! -e $R/state/host-boots ]"
ok "S35usbgadget runs boot-guard on host boots" "grep -q 'zpod-usb-role boot-guard' $S35"
ok "config.txt: Home held at power-on forces gadget (after the include)" \
	"awk '/^include zpod-usb-role.txt/{i=1} i&&/^\[gpio24=0\]/{g=1;next} g&&/^dtparam=dr_mode=peripheral/{p=1;next} p&&/^\[all\]/{a=1} END{exit !a}' $B/config.txt"
ok "config.txt: Home (GPIO24) has a pull-up" "grep -E '^gpio=.*[,=]24[,=]' $B/config.txt | grep -q 'pu'"
rm -rf "$T"
echo "$pass passed, $fail failed"
[ "$fail" -eq 0 ]
