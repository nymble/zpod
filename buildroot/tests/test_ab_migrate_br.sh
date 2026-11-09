#!/bin/bash
# Emulated in-place upgrade of a real single-system card (test5) with a real
# .zab bundle, the way scripts/zpod-ab-push runs it (build container):
#   bash buildroot/tests/test_ab_migrate_br.sh <old-sdcard.img> [bundle.zab]
# ZPOD_AB_BUSYBOX=1 runs zpod-ab under the image's own BusyBox (qemu-arm user
# mode, wrappers in /br/bbw). Partitions are files; nothing touches a card.
set -u
I=${1:?usage: $0 old-sdcard.img [bundle.zab]}
ZAB=${2:-/br/output/images/zpod-update.zab}
W=${ZPOD_AB_TEST_DIR:-/br/abmig}; rm -rf $W; mkdir -p $W/boot $W/dev $W/data $W/run
if [ -n "${ZPOD_AB_BUSYBOX:-}" ]; then
	T=${TARGET_DIR:-/br/output/target}; BB=/br/bbw; rm -rf $BB; mkdir -p $BB
	for a in $(qemu-arm -L $T $T/bin/busybox --list); do
		printf '#!/bin/sh\nexec %s -L %s %s/bin/busybox %s "$@"\n' "$(command -v qemu-arm)" $T $T $a > $BB/$a; chmod +x $BB/$a
	done
	SH="env PATH=$BB $BB/sh"
fi
read -r -a P <<< "$(sfdisk -d $I | awk -F'[=,]' '/start=/ {gsub(/ /,""); printf "%s:%s ", $2, $4}')"
MTOOLS_SKIP_CHECK=1 mcopy -s -n -i "$I@@$(( ${P[0]%%:*} * 512 ))" '::*' $W/boot/
dd if=$I of=$W/dev/mmcblk0p2 bs=512 skip=${P[1]%%:*} count=${P[1]##*:} status=none
dd if=$I of=$W/dev/mmcblk0p3 bs=512 skip=${P[2]%%:*} count=${P[2]##*:} status=none
cp -a $W/boot $W/boot.orig
p2sum=$(sha256sum < $W/dev/mmcblk0p2)
tr -d '\n' < $W/boot/cmdline.txt > $W/cmdline
echo "/dev/mmcblk0p2 / squashfs ro 0 0" > $W/mounts
export ZPOD_AB_BOOT=$W/boot ZPOD_AB_DATA=$W/data/zpod/ab ZPOD_AB_CMDLINE=$W/cmdline ZPOD_AB_DEV=$W/dev/mmcblk0p ZPOD_AB_MOUNTS=$W/mounts ZPOD_AB_RUN=$W/run ZPOD_AB_NO_GUARD=1 ZPOD_AB_REBOOT=true
SH=${SH:-sh}
pass=0 fail=0; ok() { if eval "$2"; then pass=$((pass+1)); echo "ok   $1"; else fail=$((fail+1)); echo "FAIL $1"; fi; }
echo "test5 FAT: $(ls $W/boot | tr '\n' ' ')"
ok "test5 has no slot folders / zpod-slot.txt" "[ ! -e $W/boot/slot-a ] && [ ! -e $W/boot/zpod-slot.txt ]"
# what scripts/zpod-ab-push does on the device
mkdir -p $W/data/zpod/ab && cp $ZAB $W/data/zpod/ab/incoming.zab && cd $W/data/zpod/ab && mkdir bundle && tar -xf incoming.zab -C bundle
$SH bundle/zpod-ab verify $PWD/bundle; ok "verify real bundle" "[ $? = 0 ]"
$SH bundle/zpod-ab install $PWD/bundle > $W/install.out 2>&1; r=$?; cat $W/install.out
ok "install exit 0" "[ $r = 0 ]"
sz=$(wc -c < bundle/rootfs.squashfs)
ok "p3 holds the new root" "[ \"\$(head -c $sz $W/dev/mmcblk0p3 | sha256sum)\" = \"\$(sha256sum < bundle/rootfs.squashfs)\" ]"
ok "p2 (running test5) untouched" "[ \"\$(sha256sum < $W/dev/mmcblk0p2)\" = \"$p2sum\" ]"
ok "slot-a = test5 kernel" "cmp -s $W/boot/slot-a/zImage $W/boot.orig/zImage"
ok "slot-a = test5 dtb + overlays" "cmp -s $W/boot/slot-a/bcm2708-rpi-zero-w.dtb $W/boot.orig/bcm2708-rpi-zero-w.dtb && diff -r $W/boot/slot-a/overlays $W/boot.orig/overlays >/dev/null && [ -e $W/boot/slot-a/overlays/README ]"
ok "slot-a cmdline root=p2 zpod.slot=a" "grep -q 'root=/dev/mmcblk0p2 .*zpod.slot=a\$' $W/boot/slot-a/cmdline.txt"
ok "slot-a cmdline otherwise = test5" "[ \"\$(sed 's/ zpod.slot=a//' $W/boot/slot-a/cmdline.txt)\" = \"\$(tr -d '\n' < $W/boot.orig/cmdline.txt)\" ]"
ok "slot-b = new kernel" "cmp -s $W/boot/slot-b/zImage bundle/boot/zImage"
ok "slot-b cmdline root=p3 zpod.slot=b" "grep -q 'root=/dev/mmcblk0p3 .*zpod.slot=b\$' $W/boot/slot-b/cmdline.txt"
ok "slot-b cmdline-safe" "grep -q 'root=/dev/mmcblk0p3 .*zpod.slot=b zpod.nocore zpod.safe\$' $W/boot/slot-b/cmdline-safe.txt"
ok "slot-b VERSION" "grep -qx 'version=v0.11.0-br-ab-test6' $W/boot/slot-b/VERSION"
ok "top-level kernel/cmdline still test5 (fallback)" "cmp -s $W/boot/zImage $W/boot.orig/zImage && cmp -s $W/boot/cmdline.txt $W/boot.orig/cmdline.txt"
ok "config.txt = bundle's; .prev = test5's" "cmp -s $W/boot/config.txt bundle/boot/config.txt && cmp -s $W/boot/config.txt.prev $W/boot.orig/config.txt"
ok "zpod-usb-role.txt kept" "cmp -s $W/boot/zpod-usb-role.txt $W/boot.orig/zpod-usb-role.txt"
ok "zpod-slot.txt: b, Square -> a" "[ \"\$(grep -v '^#' $W/boot/zpod-slot.txt | tr '\n' ' ')\" = 'os_prefix=slot-b/ [gpio12=0] os_prefix=slot-a/ [all] ' ]"
ok "no leftovers" "! ls -d $W/boot/*.new $W/boot/*/*.new $W/boot/slot-*.old 2>/dev/null | grep -q ."
ok "state trial=b prev=a" "grep -qx trial=b $W/data/zpod/ab/state && grep -qx prev=a $W/data/zpod/ab/state"
# first boot of B
sed 's#root=[^ ]*#root=/dev/mmcblk0p3#' $W/boot/slot-b/cmdline.txt > $W/cmdline
$SH bundle/zpod-ab boot
ok "trial boot 1 counted" "grep -qx tries=1 $W/data/zpod/ab/state"
$SH bundle/zpod-ab status | sed 's/^/  /'
$SH bundle/zpod-ab commit >/dev/null
ok "commit b" "grep -qx committed=b $W/data/zpod/ab/state && ! grep -q '^trial=' $W/data/zpod/ab/state"
# second update while on B goes to A, refreshing top level
$SH bundle/zpod-ab install $PWD/bundle >/dev/null 2>&1; ok "2nd install -> A" "grep -qx trial=a $W/data/zpod/ab/state"
ok "A top level now new" "cmp -s $W/boot/zImage bundle/boot/zImage && grep -q 'zpod.slot=a' $W/boot/cmdline.txt"
ok "p2 now new" "[ \"\$(head -c $sz $W/dev/mmcblk0p2 | sha256sum)\" = \"\$(sha256sum < bundle/rootfs.squashfs)\" ]"
ok "free on a 64 MB FAT (bytes used)" "du -sb $W/boot | awk '{print \"  used\", \$1; exit !(\$1 < 40000000)}'"
ok "log has no psk/password" "! grep -qiE 'psk|passw' $W/run/ab.log $W/install.out"
echo "$pass passed, $fail failed"; [ $fail = 0 ]
