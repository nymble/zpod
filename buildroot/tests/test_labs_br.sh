#!/bin/sh
# test7 labs and library plumbing (docs/ROADMAP-LABS.md, docs/SYNTH.md,
# docs/LIBRARY.md). Static checks on the tree; with a Buildroot target dir
# (sh test_labs_br.sh /br/output/target) also checks the built root.
# Never touches real devices.
set -u
HERE=$(cd "$(dirname "$0")" && pwd)
B=$HERE/../board/zpod
R=$HERE/../..
TGT=${1:-}
pass=0 fail=0
ok() { if eval "$2"; then pass=$((pass+1)); echo "ok   $1"; else fail=$((fail+1)); echo "FAIL $1"; fi; }
D=$HERE/../configs/zpod_pi0w_defconfig
for k in LINUX_FIRMWARE LINUX_FIRMWARE_RALINK_RT2XX LINUX_FIRMWARE_MEDIATEK_MT7601U IW BLUEZ5_UTILS_TOOLS; do
	ok "defconfig: BR2_PACKAGE_$k" "grep -qx 'BR2_PACKAGE_$k=y' $D"
done
ok "defconfig: issue says test7" "grep -q '^BR2_TARGET_GENERIC_ISSUE=.*test7' $D"
L=$B/linux.fragment
ok "S50zpod-labd executable" "[ -x $B/rootfs-overlay/etc/init.d/S50zpod-labd ]"
ok "S50zpod-labd runs the helper as root from /usr/lib/zpod/bin" "grep -q 'BIN=/usr/lib/zpod/bin/zpod-labd' $B/rootfs-overlay/etc/init.d/S50zpod-labd && ! grep -q -- '-c 1000' $B/rootfs-overlay/etc/init.d/S50zpod-labd"
ok "build-arm: labd in core" "grep -q 'zpod-app zpod-labd; do cp' $R/apps/tools/build-arm.sh"
for a in usb-lab ble-lab wifi-lab synth; do
	ok "build-arm bundles $a" "grep -q 'for a in .* $a' $R/apps/tools/build-arm.sh"
	ok "package $a: manifest + 32x32 icon" "[ -f $R/apps/packages/$a/manifest.toml ] && [ \"\$(od -An -tx1 -j16 -N8 $R/apps/packages/$a/icon.png | tr -d ' ')\" = 0000002000000020 ]"
done
ok "usb-lab under Tools" "grep -qx 'path = \"Tools\"' $R/apps/packages/usb-lab/manifest.toml"
ok "ble-lab under Tools" "grep -qx 'path = \"Tools\"' $R/apps/packages/ble-lab/manifest.toml"
ok "wifi-lab under Tools/Radio" "grep -qx 'path = \"Tools/Radio\"' $R/apps/packages/wifi-lab/manifest.toml"
ok "synth under Generative" "grep -qx 'path = \"Generative\"' $R/apps/packages/synth/manifest.toml"
M=$B/rootfs-overlay/etc/mpd.conf
for f in tag_cache state sticker.sql; do ok "mpd.conf: $f on /data" "grep -q '\"/data/var/mpd/$f\"' $M"; done
ok "zpod-data-early creates /data/var/mpd" "grep -q '/data/var/mpd' $B/rootfs-overlay/usr/sbin/zpod-data-early"
G=$R/overlay/usr/lib/zpod/zpod-usb-gadget.sh
ok "gadget: MIDI 2 in + 2 out" "grep -q '^MIDI_PORTS=2' $G"
ok "gadget: /data MIDI switch" "grep -q 'MIDI_CFG=.*/data/zpod/usb-midi.txt' $G"
ok "usb-role: midi command" "grep -q '^midi) ' $B/rootfs-overlay/usr/sbin/zpod-usb-role"
# zpod-bundled-new: new bundled apps on an updated card, removed ones stay removed
BN=$B/rootfs-overlay/usr/lib/zpod/zpod-bundled-new
W=$(mktemp -d); mkdir -p $W/pkgs $W/apps/music $W/apps/gps
for f in music-1.2.0 clock-1.2.0 tetris-1.1.0 gps-0.2.0 usb-lab-0.1.0 ble-lab-0.1.0 wifi-lab-0.1.0 synth-0.1.0; do : > $W/pkgs/$f.zpa; done
printf '#!/bin/sh\nb=$(basename "$2" .zpa); mkdir -p "$ZPOD_APPS_DIR/${b%%%%-[0-9]*}"; echo "$b" >> "$ZPOD_APPS_DIR/.log"\n' > $W/fakeapp; chmod +x $W/fakeapp
bn() { ZPOD_BUNDLED_DIR=$W/pkgs ZPOD_BUNDLED_LIST=$W/list ZPOD_APPS_DIR=$W/apps ZPOD_APP=$W/fakeapp sh $BN; }
bn >/dev/null
ok "bundled-new: test6 card gets the 4 new apps" "[ -d $W/apps/usb-lab ] && [ -d $W/apps/ble-lab ] && [ -d $W/apps/wifi-lab ] && [ -d $W/apps/synth ]"
ok "bundled-new: removed test6-era apps stay removed (clock, tetris)" "[ ! -e $W/apps/clock ] && [ ! -e $W/apps/tetris ]"
rm -rf $W/apps/synth; bn >/dev/null
ok "bundled-new: a removed new app is not reinstalled" "[ ! -e $W/apps/synth ] && [ \$(wc -l < $W/apps/.log) = 4 ]"
ok "bundled-new: ids recorded once" "[ \$(sort $W/list | uniq -d | wc -l) = 0 ] && grep -qx wifi-lab $W/list"
ok "S00zpod-data runs zpod-bundled-new" "grep -q zpod-bundled-new $B/rootfs-overlay/etc/init.d/S00zpod-data"
rm -rf $W
if [ -n "$TGT" ]; then
	ok "target: rt2870.bin" "[ -f $TGT/lib/firmware/rt2870.bin ]"
	ok "target: mt7601u.bin" "[ -f $TGT/lib/firmware/mediatek/mt7601u.bin ]"
	ok "target: brcm firmware still there" "ls $TGT/lib/firmware/brcm/brcmfmac43430-sdio* >/dev/null 2>&1"
	ok "target: zpod-labd" "[ -x $TGT/usr/lib/zpod/bin/zpod-labd ]"
	ok "target: S50zpod-labd" "[ -x $TGT/etc/init.d/S50zpod-labd ]"
	for a in usb-lab ble-lab wifi-lab synth; do ok "target: bundled $a" "ls $TGT/usr/share/zpod/packages/$a-*.zpa >/dev/null 2>&1"; done
	for m in rt2800usb mt7601u snd-usb-audio; do ok "target: module $m" "find $TGT/lib/modules -name '$m.ko*' | grep -q ."; done
	ok "target: core-version 0.12.0" "[ \"\$(cat $TGT/etc/zpod/core-version)\" = 0.12.0 ]"
	ok "target: no injection tool shipped (aireplay/mdk)" "! ls $TGT/usr/bin/aireplay-ng $TGT/usr/bin/mdk3 $TGT/usr/bin/mdk4 >/dev/null 2>&1"
fi
echo "$pass passed, $fail failed"
[ "$fail" -eq 0 ]
