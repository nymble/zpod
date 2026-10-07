#!/usr/bin/env bash
# Offline checks of an M2 .img (never flashes anything). Mounts the root
# partition READ-ONLY via losetup --offset and reads the FAT with mtools
# (IMG@@offset), then runs the Python imports / dkms status in a qemu chroot.
# Usage: sudo scripts/verify-m2-image.sh path/to/image.img
set -euo pipefail
IMG="${1:?usage: $0 image.img}"
pass=0; fail=0
ok()  { echo "PASS  $*"; pass=$((pass + 1)); }
bad() { echo "FAIL  $*"; fail=$((fail + 1)); }
chk() { local d="$1"; shift; if "$@" >/dev/null 2>&1; then ok "$d"; else bad "$d"; fi; }

read -r BOOT_START ROOT_START ROOT_SIZE < <(parted -m -s "$IMG" unit B print |
  awk -F: '$1=="1"{b=$2} $1=="2"{r=$2; s=$4} END{gsub("B","",b); gsub("B","",r); gsub("B","",s); print b, r, s}')
BOOT="${IMG}@@${BOOT_START}"
TMP=$(mktemp -d)
MNT="$TMP/root"; mkdir -p "$MNT"
LOOP=""
cleanup() {
  set +e
  for m in sys proc dev; do umount -l "$MNT/$m" 2>/dev/null; done
  umount -l "$MNT/tmp" 2>/dev/null
  umount -l "$MNT" 2>/dev/null
  [[ -n "$LOOP" ]] && losetup -d "$LOOP"
  rm -rf "$TMP"
}
trap cleanup EXIT

mt() { MTOOLS_SKIP_CHECK=1 "$@"; }
mt mcopy -n -i "$BOOT" ::config.txt "$TMP/config.txt"
mt mcopy -n -i "$BOOT" ::cmdline.txt "$TMP/cmdline.txt"
echo "--- cmdline.txt: $(cat "$TMP/cmdline.txt")"
chk "cmdline: no serial console"          bash -c "! grep -q 'console=serial0' '$TMP/cmdline.txt'"
chk "cmdline: root=PARTUUID"              grep -q 'root=PARTUUID=' "$TMP/cmdline.txt"
chk "config: dtparam=i2c_arm=on"          grep -qx 'dtparam=i2c_arm=on' "$TMP/config.txt"
chk "config: dtparam=i2s=on"              grep -qx 'dtparam=i2s=on' "$TMP/config.txt"
chk "config: dtparam=spi=on"              grep -qx 'dtparam=spi=on' "$TMP/config.txt"
chk "config: dtparam=audio=off"           grep -qx 'dtparam=audio=off' "$TMP/config.txt"
chk "config: no dtparam=audio=on"         bash -c "! grep -qx 'dtparam=audio=on' '$TMP/config.txt'"
chk "config: dtoverlay=aoide-zpod-dac"    grep -qx 'dtoverlay=aoide-zpod-dac' "$TMP/config.txt"
chk "config: gpio=14=op,dl"               grep -qx 'gpio=14=op,dl' "$TMP/config.txt"
chk "config: button pull-ups"             grep -qx 'gpio=4,5,6,12,13,16,17,22,23,24,26=ip,pu' "$TMP/config.txt"
chk "config: dwc2 peripheral"             grep -qx 'dtoverlay=dwc2,dr_mode=peripheral' "$TMP/config.txt"
# base-DT dtparams must come before the first dtoverlay= line
first_ov=$(grep -n '^dtoverlay=' "$TMP/config.txt" | head -1 | cut -d: -f1)
last_dp=$(grep -nE '^dtparam=(i2c_arm|i2s|spi|audio)=' "$TMP/config.txt" | tail -1 | cut -d: -f1)
chk "config: base dtparams before first dtoverlay (line $last_dp < $first_ov)" test "$last_dp" -lt "$first_ov"
chk "boot: overlays/aoide-zpod-dac.dtbo"  bash -c "MTOOLS_SKIP_CHECK=1 mdir -i '$BOOT' ::overlays/aoide-zpod-dac.dtbo"
chk "boot: overlays/dwc2.dtbo"            bash -c "MTOOLS_SKIP_CHECK=1 mdir -i '$BOOT' ::overlays/dwc2.dtbo"
mt mcopy -n -i "$BOOT" ::overlays/aoide-zpod-dac.dtbo "$TMP/a.dtbo"
echo "--- dtbo sha256: $(sha256sum "$TMP/a.dtbo" | cut -c1-64)"
chk "boot: dtbo == vendor sha256 3ad223aa…" bash -c "sha256sum '$TMP/a.dtbo' | grep -q ^3ad223aa90bc893b8dc1c47a561ace1787e97e9002400c3652f14c19403ce069"

LOOP=$(losetup -f --show -r --offset "$ROOT_START" --sizelimit "$ROOT_SIZE" "$IMG")
mount -o ro,noload -t ext4 "$LOOP" "$MNT"
R="$MNT"
for k in "$R"/lib/modules/*-rpi-v6 "$R"/lib/modules/*-rpi-v7; do
  kv=$(basename "$k")
  chk "dkms module for $kv" bash -c "ls '$k'/updates/dkms/aoide-zpod-dac.ko*"
done
chk "dkms source /usr/src/aoide-zpod-dac-1.0.0" test -f "$R/usr/src/aoide-zpod-dac-1.0.0/dkms.conf"
for u in zpod-ui zpod-buttons zpod-audio-init rpi-usb-gadget-ics; do
  chk "service enabled: $u" test -L "$R/etc/systemd/system/multi-user.target.wants/$u.service"
done
chk "asound.conf default = DAC"           grep -q 'sndrpiaoidezpod' "$R/etc/asound.conf"
chk "NM wifi.powersave = 2"               grep -q 'wifi.powersave = 2' "$R/etc/NetworkManager/conf.d/90-zpod-wifi-powersave.conf"
chk "NM preconfigured powersave=2"        grep -q '^powersave=2' "$R/etc/NetworkManager/system-connections/preconfigured.nmconnection"
chk "NM usb0 shared profile (600)"        bash -c "[ \"\$(stat -c %a '$R/etc/NetworkManager/system-connections/usb-gadget-shared.nmconnection')\" = 600 ]"
chk "usb0 NM-managed udev rule"           test -f "$R/etc/udev/rules.d/86-zpod-usb-gadget-nm-managed.rules"
chk "g_ether loaded at boot"              grep -qx g_ether "$R/etc/modules-load.d/usb-gadget.conf"
chk "uinput loaded at boot"               grep -qx uinput "$R/etc/modules-load.d/zpod.conf"
chk "journald persistent"                 grep -q 'Storage=persistent' "$R/etc/systemd/journald.conf.d/90-zpod-persistent.conf"
chk "/var/log/journal exists"             test -d "$R/var/log/journal"
for b in i2cdetect gpioinfo gpioget iw amixer speaker-test dkms evtest; do
  if [[ -x "$R/usr/bin/$b" || -x "$R/usr/sbin/$b" ]]; then ok "tool: $b"; else
    [[ "$b" == evtest ]] && echo "info  tool: evtest not installed (optional)" || bad "tool: $b"; fi
done
chk "no desktop (no lightdm / xserver)"   bash -c "! ls '$R'/usr/sbin/lightdm '$R'/usr/lib/xorg/Xorg 2>/dev/null | grep -q ."
chk "pi has no baked password"            bash -c "grep '^pi:' '$R/etc/shadow' | cut -d: -f2 | grep -qE '^(\*|!|!\*|)$'"
chk "userconfig.service enabled"          bash -c "ls '$R'/etc/systemd/system/*.wants/userconfig.service"

# qemu chroot: imports + dkms status (read-only root; tmpfs /tmp)
if command -v qemu-arm-static >/dev/null && [[ -e /proc/sys/fs/binfmt_misc/qemu-arm ]]; then
  mount -t tmpfs tmpfs "$MNT/tmp"
  for m in dev proc sys; do mount --bind "/$m" "$MNT/$m"; done
  out=$(chroot "$MNT" /usr/bin/env PYTHONDONTWRITEBYTECODE=1 python3 -c "
import spidev, lgpio, evdev, PIL, smbus2, sys
sys.path.insert(0, '/usr/lib/zpod')
import zpod_hw, zpod_lcd
from evdev import ecodes as e
import importlib.util
for m in ('zpod_ui', 'zpod_buttons'):
    spec = importlib.util.spec_from_file_location(m, '/usr/lib/zpod/%s.py' % m)
    mod = importlib.util.module_from_spec(spec); spec.loader.exec_module(mod)
img = PIL.Image.new('RGB', (320, 240), (255, 0, 0))
b = zpod_lcd.to_rgb565(img)
assert len(b) == 153600 and b[:2] == bytes([0xF8, 0x00]), b[:2]
print('imports ok; PIL', PIL.__version__, '; evdev KEY_ENTER', e.KEY_ENTER)
" 2>&1) && ok "chroot python: $out" || bad "chroot python: $out"
  ds=$(chroot "$MNT" dkms status 2>&1 | tr '\n' ';')
  echo "--- dkms status: $ds"
  [[ "$ds" == *"rpi-v6"*"installed"* ]] && ok "dkms status shows rpi-v6 installed" || bad "dkms status"
  mi=$(chroot "$MNT" modinfo -k "$(basename "$R"/lib/modules/*-rpi-v6)" aoide-zpod-dac 2>&1 | grep -E '^(filename|vermagic|alias|license)' | tr '\n' ';')
  echo "--- modinfo (v6): $mi"
  [[ "$mi" == *"aoide,aoide-zpod-dac"* ]] && ok "modinfo alias of:...aoide,aoide-zpod-dac" || bad "modinfo"
  chroot "$MNT" systemd-analyze verify --man=no /usr/lib/systemd/system/zpod-ui.service \
    /usr/lib/systemd/system/zpod-buttons.service /usr/lib/systemd/system/zpod-audio-init.service \
    >"$TMP/verify.txt" 2>&1 && ok "systemd-analyze verify units" || { echo "info  systemd-analyze: $(grep -i zpod "$TMP/verify.txt" | head -5 | tr '\n' ' ')"; }
fi

echo "=== $pass passed, $fail failed"
[[ "$fail" -eq 0 ]]
