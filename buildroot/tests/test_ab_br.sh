#!/bin/sh
# A/B system updates (zpod-ab, docs/AB-UPDATES.md) on a fake card: the FAT
# boot partition is a folder, p2 / p3 are files, /proc/cmdline and
# /proc/mounts are files. Covers bundle checks, the test5 -> A/B migration,
# trial boots, commit by the launcher guard, automatic and manual fallback,
# and a second update back into system A. Never touches a real device.
set -u
HERE=$(cd "$(dirname "$0")" && pwd)
AB=$HERE/../board/zpod/rootfs-overlay/usr/sbin/zpod-ab
pass=0 fail=0
ok() { if eval "$2"; then pass=$((pass+1)); echo "ok   $1"; else fail=$((fail+1)); echo "FAIL $1"; fi; }
T=$(mktemp -d)
sum() { sha256sum "$1" | cut -d' ' -f1; }
card() { # a test5 card: single system, top-level boot files, root=p2
	rm -rf "$T/boot" "$T/dev" "$T/data" "$T/run"; mkdir -p "$T/boot/overlays" "$T/dev" "$T/data" "$T/run"
	echo OLDKERNEL > "$T/boot/zImage"; echo OLDDTB > "$T/boot/bcm2708-rpi-zero-w.dtb"
	echo readme > "$T/boot/overlays/README"; echo olddtbo > "$T/boot/overlays/old.dtbo"
	echo "root=/dev/mmcblk0p2 rootfstype=squashfs ro rootwait panic=5" > "$T/boot/cmdline.txt"
	echo "root=/dev/mmcblk0p2 rootfstype=squashfs ro rootwait panic=5 zpod.nocore zpod.safe" > "$T/boot/cmdline-safe.txt"
	printf '# ZPOD test5\ninclude zpod-usb-role.txt\n[gpio24=0]\ncmdline=cmdline-safe.txt\n[all]\n' > "$T/boot/config.txt"
	echo '#dtparam=dr_mode=host' > "$T/boot/zpod-usb-role.txt"
	printf 'OLDROOT' > "$T/dev/p2"; truncate -s 1M "$T/dev/p2"
	printf 'OLDROOT' > "$T/dev/p3"; truncate -s 1M "$T/dev/p3"
	echo "/dev/root / squashfs ro 0 0" > "$T/mounts"
	boot_from 2
}
boot_from() { echo "root=/dev/mmcblk0p$1 rootfstype=squashfs ro rootwait panic=5" > "$T/cmdline"; }
bundle() { # version kernel [rootfs-bytes]
	D=$T/bundle; rm -rf "$D"; mkdir -p "$D/boot/overlays"
	{ printf 'hsqs'; head -c "${3:-300000}" /dev/urandom; } > "$D/rootfs.squashfs"
	echo "$2" > "$D/boot/zImage"; echo NEWDTB > "$D/boot/bcm2708-rpi-zero-w.dtb"
	echo readme > "$D/boot/overlays/README"; echo newdtbo > "$D/boot/overlays/new.dtbo"
	echo "root=/dev/mmcblk0p2 rootfstype=squashfs ro rootwait quiet panic=5" > "$D/boot/cmdline.txt"
	printf '# ZPOD %s\ninclude zpod-usb-role.txt\n[gpio24=0]\ncmdline=cmdline-safe.txt\n[all]\ninclude zpod-slot.txt\n' "$1" > "$D/boot/config.txt"
	cp "$AB" "$D/zpod-ab"
	manifest "$1"
}
manifest() {
	{
		printf 'format=1\nboard=pi0w\nversion=%s\ngit=abc%s\ndate=2026-11-06\nabi=1\n' "$1" "$(echo "$1" | tr -d .)"
		(cd "$D" && find . -type f ! -name AB.manifest | sort | while read -r f; do echo "file=$(sha256sum "$f" | cut -d' ' -f1) ${f#./}"; done)
	} > "$D/AB.manifest"
}
ab() {
	ZPOD_AB_BOOT=$T/boot ZPOD_AB_DATA=$T/data ZPOD_AB_CMDLINE=$T/cmdline ZPOD_AB_DEV=$T/dev/p ZPOD_AB_MOUNTS=$T/mounts \
	ZPOD_AB_RUN=$T/run ZPOD_AB_SOCK=$T/run/launcher.sock ZPOD_AB_FAKE_PID=$T/run/pid ZPOD_AB_GUARD_SECS=2 ZPOD_AB_SETTLE=1 \
	ZPOD_AB_NO_GUARD=1 ZPOD_AB_REBOOT="touch $T/rebooted" sh "$AB" "$@"
}
kvs() { ab status --kv | sed -n "s/^$1=//p"; }
st() { sed -n "s/^$1=//p" "$T/data/state"; }
first() { sed -n "s/^os_prefix=//p" "$T/boot/zpod-slot.txt" | sed -n "${1}p"; }
same_root() { [ "$(head -c "$(wc -c < "$T/bundle/rootfs.squashfs")" "$T/dev/p$1" | sha256sum | cut -d' ' -f1)" = "$(sum "$T/bundle/rootfs.squashfs")" ]; }

ok "script is executable, sh -n clean" "[ -x $AB ] && sh -n $AB"

card
ok "status on a test5 card: system a, single layout" "[ \"\$(kvs slot)\" = a ] && [ \"\$(kvs layout)\" = single ] && kvs a | grep -q top-level"

bundle 0.11.0 NEWKERNEL1
ok "verify: good bundle folder" "ab verify $T/bundle | grep -q '^ok: 0.11.0'"
(cd "$T/bundle" && tar -cf "$T/b.zab" AB.manifest zpod-ab rootfs.squashfs boot)
ok "verify: good .zab tar" "ab verify $T/b.zab | grep -q '^ok: 0.11.0' && [ ! -d $T/data/unpacked ]"
echo tampered >> "$T/bundle/boot/zImage"
ok "verify: refuses a file that does not match its sha256" "! ab verify $T/bundle >/dev/null && ab verify $T/bundle | grep -q 'zImage does not match'"
bundle 0.11.0 NEWKERNEL1; rm "$T/bundle/boot/overlays/README"; manifest 0.11.0
ok "verify: refuses a bundle without overlays/README" "ab verify $T/bundle | grep -q 'README is not listed'"
bundle 0.11.0 NEWKERNEL1; printf 'nope' > "$T/bundle/rootfs.squashfs"; manifest 0.11.0
ok "verify: refuses a root that is not squashfs" "ab verify $T/bundle | grep -q 'not a squashfs'"
bundle 0.11.0 NEWKERNEL1; sed -i 's/^include zpod-slot.txt//' "$T/bundle/boot/config.txt"; manifest 0.11.0
ok "verify: refuses a config.txt without the zpod-slot.txt include" "ab verify $T/bundle | grep -q 'lacks'"
bundle 0.11.0 NEWKERNEL1; sed -i 's/^board=.*/board=pi4/' "$T/bundle/AB.manifest"
ok "verify: refuses another board" "ab verify $T/bundle | grep -q 'board'"
bundle 0.11.0 NEWKERNEL1; echo "file=$(sum "$T/bundle/zpod-ab") ../zpod-ab" >> "$T/bundle/AB.manifest"
ok "verify: refuses paths outside the bundle" "ab verify $T/bundle | grep -q 'bad path'"

# refusals change nothing
bundle 0.11.0 NEWKERNEL1 2000000
ok "install: refuses a root larger than the partition" "! ab install $T/bundle >/dev/null 2>&1 && [ ! -e $T/boot/zpod-slot.txt ] && head -c 7 $T/dev/p3 | grep -q OLDROOT"
bundle 0.11.0 NEWKERNEL1
echo "$T/dev/p3 /mnt squashfs ro 0 0" >> "$T/mounts"
ok "install: refuses a mounted target" "! ab install $T/bundle >/dev/null 2>&1 && [ ! -e $T/boot/zpod-slot.txt ]"
echo "/dev/root / squashfs ro 0 0" > "$T/mounts"

# test5 -> A/B migration, running on system a
out=$(ab install "$T/bundle" 2>&1)
ok "install: succeeds and says what it did" "echo \"\$out\" | grep -q 'next boot runs system b (0.11.0)'"
ok "install: p3 holds the new root, read back" "same_root 3"
ok "install: p2 (the running system) untouched" "head -c 7 $T/dev/p2 | grep -q OLDROOT"
ok "install: slot-b has the new kernel, dtb, overlays" "grep -q NEWKERNEL1 $T/boot/slot-b/zImage && grep -q NEWDTB $T/boot/slot-b/bcm2708-rpi-zero-w.dtb && [ -f $T/boot/slot-b/overlays/README ] && [ -f $T/boot/slot-b/overlays/new.dtbo ]"
ok "install: slot-b command line boots p3" "grep -q 'root=/dev/mmcblk0p3 .* zpod.slot=b\$' $T/boot/slot-b/cmdline.txt && ! grep -q mmcblk0p2 $T/boot/slot-b/cmdline.txt"
ok "install: slot-b safe command line (Home at power-on)" "grep -q 'root=/dev/mmcblk0p3 .*zpod.nocore zpod.safe' $T/boot/slot-b/cmdline-safe.txt"
ok "install: slot-a = the running system's files (migration)" "grep -q OLDKERNEL $T/boot/slot-a/zImage && grep -q 'root=/dev/mmcblk0p2.*zpod.slot=a' $T/boot/slot-a/cmdline.txt && [ -f $T/boot/slot-a/overlays/old.dtbo ]"
ok "install: top-level files (no-prefix fallback = system a) untouched" "grep -q OLDKERNEL $T/boot/zImage && grep -q 'root=/dev/mmcblk0p2' $T/boot/cmdline.txt"
ok "install: config.txt from the bundle, previous kept" "grep -q '^include zpod-slot.txt' $T/boot/config.txt && grep -q test5 $T/boot/config.txt.prev"
ok "install: USB role file kept" "grep -q dr_mode $T/boot/zpod-usb-role.txt"
ok "install: zpod-slot.txt boots b, Square boots a" "[ \"\$(first 1)\" = slot-b/ ] && [ \"\$(first 2)\" = slot-a/ ] && grep -q '^\[gpio12=0\]' $T/boot/zpod-slot.txt"
ok "install: state = trial of b, prev a" "[ \"\$(st trial)\" = b ] && [ \"\$(st prev)\" = a ] && [ \"\$(st tries)\" = 0 ]"
ok "install: no leftovers (.new, unpacked)" "! ls -d $T/boot/*.new $T/boot/slot-*.new $T/boot/slot-*.old >/dev/null 2>&1"
ok "status: layout ab, next b, versions" "[ \"\$(kvs layout)\" = ab ] && [ \"\$(kvs next)\" = b ] && kvs b | grep -q '0.11.0 abc0110'"

# boot b: trial; the launcher comes up -> committed
boot_from 3; rm -f "$T/rebooted"
ab boot
ok "boot b: trial boot 1 of 2" "[ \"\$(st tries)\" = 1 ] && grep -qx slot=b $T/run/ab-slot"
ok "install on trial: refused (keeps the fallback)" "! ab install $T/bundle >/dev/null 2>&1 && head -c 7 $T/dev/p2 | grep -q OLDROOT"
echo 4242 > "$T/run/pid"; : > "$T/run/launcher.sock"
ab guard
ok "guard: launcher healthy -> committed b, trial over" "[ \"\$(st committed)\" = b ] && [ -z \"\$(st trial)\" ] && [ ! -e $T/rebooted ] && [ \"\$(first 1)\" = slot-b/ ]"
ab boot
ok "boot after commit: nothing to do" "[ -z \"\$(st trial)\" ] && [ \"\$(st committed)\" = b ]"

# second update, running b: goes to system a (p2) and the top-level files
bundle 0.12.0 NEWKERNEL2
ab install "$T/bundle" >/dev/null 2>&1
ok "2nd install from b: p2 written, p3 (running) kept" "same_root 2 && ! same_root 3"
ok "2nd install: slot-a + top-level files are the new system a" "grep -q NEWKERNEL2 $T/boot/slot-a/zImage && grep -q NEWKERNEL2 $T/boot/zImage && grep -q 'root=/dev/mmcblk0p2 .*zpod.slot=a' $T/boot/cmdline.txt"
ok "2nd install: slot-b untouched" "grep -q NEWKERNEL1 $T/boot/slot-b/zImage"
ok "2nd install: next a, Square b" "[ \"\$(first 1)\" = slot-a/ ] && [ \"\$(first 2)\" = slot-b/ ] && [ \"\$(st trial)\" = a ]"

# trial a: the launcher never comes up -> back to b + reboot
boot_from 2; rm -f "$T/rebooted" "$T/run/pid" "$T/run/launcher.sock"
ab boot; ab guard >/dev/null
ok "guard: launcher not up -> back to b, reboot" "[ \"\$(first 1)\" = slot-b/ ] && [ -e $T/rebooted ] && [ -z \"\$(st trial)\" ] && st result | grep -q 'not up within 2 s'"

# trial that never reaches the guard: the boot counter gives up
boot_from 3; ab boot   # rebooted into b: no trial
bundle 0.12.1 NEWKERNEL3; ab install "$T/bundle" >/dev/null 2>&1
boot_from 2; rm -f "$T/rebooted"
ab boot; ab boot
ok "boot count: 2 trial boots allowed" "[ \"\$(st tries)\" = 2 ] && [ ! -e $T/rebooted ]"
ab boot
ok "boot count: 3rd -> back to b, reboot" "[ \"\$(first 1)\" = slot-b/ ] && [ -e $T/rebooted ] && st result | grep -q 'did not start on 2 boots'"

# trial, but the old system was booted (Square held / unusable folder)
bundle 0.12.2 NEWKERNEL4; boot_from 3; ab install "$T/bundle" >/dev/null 2>&1
rm -f "$T/rebooted"; boot_from 3; ab boot
ok "booted the old system during a trial -> trial abandoned, stays" "[ -z \"\$(st trial)\" ] && [ \"\$(first 1)\" = slot-b/ ] && [ ! -e $T/rebooted ] && st result | grep -q 'was not booted'"

# by hand
ok "switch: next boot the other system" "ab switch | grep -q 'next boot runs system a' && [ \"\$(first 1)\" = slot-a/ ]"
ok "commit: keep the running one" "ab commit | grep -q 'ok: system b' && [ \"\$(first 1)\" = slot-b/ ]"
card
ok "switch: refused without the other folder" "! ab switch >/dev/null 2>&1 && [ ! -e $T/boot/zpod-slot.txt ]"
ok "log has no passwords / psk" "! grep -riq 'psk\\|password' $T/run/ab.log 2>/dev/null"

rm -rf "$T"
echo "$pass passed, $fail failed"
[ "$fail" = 0 ]
