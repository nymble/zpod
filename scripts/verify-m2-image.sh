#!/usr/bin/env bash
# Offline checks of an M2/M3/M4/M5/M6 .img (never flashes anything). Mounts the root
# partition READ-ONLY via losetup --offset and reads the FAT with mtools
# (IMG@@offset), then runs the Python imports / dkms status in a qemu chroot.
# Usage: sudo scripts/verify-m2-image.sh path/to/image.img
set -euo pipefail
IMG="${1:?usage: $0 image.img}"
pass=0; fail=0
ok()  { echo "PASS  $*"; pass=$((pass + 1)); }
bad() { echo "FAIL  $*"; fail=$((fail + 1)); }
chk() { local d="$1"; shift; if "$@" >/dev/null 2>&1; then ok "$d"; else bad "$d"; fi; }

read -r BOOT_START ROOT_START ROOT_SIZE DATA_START DATA_SIZE < <(parted -m -s "$IMG" unit B print |
  awk -F: '$1=="1"{b=$2} $1=="2"{r=$2; s=$4} $1=="3"{d=$2; ds=$4} END{gsub("B","",b); gsub("B","",r); gsub("B","",s); gsub("B","",d); gsub("B","",ds); print b, r, s, (d==""?0:d), (ds==""?0:ds)}')
BOOT="${IMG}@@${BOOT_START}"
TMP=$(mktemp -d)
MNT="$TMP/root"; mkdir -p "$MNT"
LOOP=""
DLOOP=""
cleanup() {
  set +e
  for m in sys proc dev; do umount -l "$MNT/$m" 2>/dev/null; done
  umount -l "$MNT/tmp" 2>/dev/null
  umount -l "$MNT" 2>/dev/null
  [[ -n "$DLOOP" ]] && { umount -l "$TMP/data" 2>/dev/null; losetup -d "$DLOOP"; }
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
M4=0; [[ -x "$R/usr/lib/zpod/bin/zpod-launcher" ]] && M4=1
UI=zpod-ui; [[ $M4 = 1 ]] && UI=zpod-launcher
for u in $UI zpod-buttons zpod-audio-init rpi-usb-gadget-ics; do
  chk "service enabled: $u" bash -c "ls '$R'/etc/systemd/system/multi-user.target.wants/$u.service"
done
chk "asound.conf default = DAC"           grep -q 'sndrpiaoidezpod' "$R/etc/asound.conf"
chk "NM wifi.powersave = 2"               grep -q 'wifi.powersave = 2' "$R/etc/NetworkManager/conf.d/90-zpod-wifi-powersave.conf"
chk "NM usb0 shared profile (600)"        bash -c "[ \"\$(stat -c %a '$R/etc/NetworkManager/system-connections/usb-gadget-shared.nmconnection')\" = 600 ]"
chk "usb0 NM-managed udev rule"           test -f "$R/etc/udev/rules.d/86-zpod-usb-gadget-nm-managed.rules"
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

# --- M3 additions ---------------------------------------------------------
chk "M3: zpod-usb-kick enabled (usb0 device wants)" test -L "$R/etc/systemd/system/sys-subsystem-net-devices-usb0.device.wants/zpod-usb-kick.service"
chk "M3: zpod-usb-kick.sh executable"      test -x "$R/usr/lib/zpod/zpod-usb-kick.sh"
chk "M3: i2c-dev loaded at boot"            grep -qx i2c-dev "$R/etc/modules-load.d/zpod.conf"
chk "M3: shutdown hook zpod-lcd-off (755)"  bash -c "[ \"\$(stat -c %a '$R/usr/lib/systemd/system-shutdown/zpod-lcd-off')\" = 755 ]"
chk "M3: ui.conf shutdown screen = message" grep -qx 'screen = message' "$R/etc/zpod/ui.conf"
chk "M3: zpod-ui end_screen()"              grep -q 'def end_screen' "$R/usr/lib/zpod/zpod_ui.py"
chk "M3: zpod_lcd blank_off()"              grep -q 'def blank_off' "$R/usr/lib/zpod/zpod_lcd.py"
chk "M4: volume repeat delay 650 ms"         grep -q '^VOL_REPEAT_DELAY_S = 0.650' "$R/usr/lib/zpod/zpod_buttons.py"
chk "M4: volume cap 80 %"                     grep -qx 'VOL_CAP_PCT = 80' "$R/usr/lib/zpod/zpod_buttons.py"
for u in apt-daily.timer apt-daily-upgrade.timer console-setup.service; do
  chk "M3: $u masked"                       bash -c "[ \"\$(readlink '$R/etc/systemd/system/$u')\" = /dev/null ]"
done
chk "M3: NetworkManager-wait-online disabled" bash -c "! ls '$R'/etc/systemd/system/*.wants/NetworkManager-wait-online.service 2>/dev/null | grep -q ."
chk "no build-host APT proxy (51cache)"    test ! -e "$R/etc/apt/apt.conf.d/51cache"
chk "apt sources = official raspbian"      grep -q 'raspbian.raspberrypi.com' "$R/etc/apt/sources.list.d/raspbian.sources"
chk "M3: udisks2 not enabled"               bash -c "! ls '$R'/etc/systemd/system/*.wants/udisks2.service 2>/dev/null | grep -q ."

# --- M3.1: no Wi-Fi credentials in the image; first-boot import instead -----
NMC="$R/etc/NetworkManager/system-connections"
extra=$(ls -A "$NMC" 2>/dev/null | { grep -vxE 'usb-gadget-(shared|client)\.nmconnection' || true; } | tr '\n' ' ')
chk "M3.1: system-connections holds only the usb0 profiles${extra:+ (extra: $extra)}" test -z "$extra"
chk "M3.1: no Wi-Fi/PSK in any .nmconnection" bash -c "! grep -lsE '^(type=(wifi|802-11-wireless)|psk=|\[wifi-security\])' '$NMC'/*.nmconnection | grep -q ."
chk "M3.1: no wpa_supplicant config with a psk (rootfs)" bash -c "! grep -rlsE '^[[:space:]]*(psk|wep_key[0-9]|password)=' '$R/etc/wpa_supplicant' '$R/boot/firmware' | grep -q ."
bootls=$(MTOOLS_SKIP_CHECK=1 mdir -b -i "$BOOT" :: 2>/dev/null | tr 'A-Z' 'a-z')
chk "M3.1: boot FAT has no wpa_supplicant.conf / zpod-wifi.txt / *.nmconnection" \
  bash -c "! printf '%s\n' \"\$1\" | grep -qE '(wpa_supplicant\.conf|zpod-wifi\.txt|\.nmconnection)\$'" _ "$bootls"
chk "M3.1: zpod-wifi-import enabled"         test -L "$R/etc/systemd/system/multi-user.target.wants/zpod-wifi-import.service"
chk "M3.1: zpod-wifi-import.sh executable"   test -x "$R/usr/lib/zpod/zpod-wifi-import.sh"
chk "M3.1: import runs before NetworkManager" grep -q '^Before=NetworkManager.service' "$R/usr/lib/systemd/system/zpod-wifi-import.service"

# --- M4 system fixes ------------------------------------------------------
chk "M4: config dtoverlay=i2c-rtc,ds3231"     grep -qx 'dtoverlay=i2c-rtc,ds3231' "$TMP/config.txt"
chk "M4: boot zpod-usb.txt mode=ncm"          bash -c "MTOOLS_SKIP_CHECK=1 mtype -i '$BOOT' ::zpod-usb.txt | grep -qx 'mode=ncm'"
chk "M4: g_ether NOT loaded at boot"          bash -c "! grep -rqsx g_ether '$R'/etc/modules-load.d/"
chk "M4: no g_ether modprobe options"         test ! -e "$R/etc/modprobe.d/zpod-g_ether.conf"
chk "M4: zpod-usb-gadget enabled"             test -L "$R/etc/systemd/system/sysinit.target.wants/zpod-usb-gadget.service"
chk "M4: zpod-usb-gadget.sh executable"       test -x "$R/usr/lib/zpod/zpod-usb-gadget.sh"
chk "M4: usb_f_ncm module (rpi-v6)"           bash -c "ls '$R'/lib/modules/*-rpi-v6/kernel/drivers/usb/gadget/function/usb_f_ncm.ko* || grep -q usb_f_ncm '$R'/lib/modules/*-rpi-v6/modules.builtin"
chk "M4: zpod-rtc enabled"                    test -L "$R/etc/systemd/system/sysinit.target.wants/zpod-rtc.service"
chk "M4: zpod-rtc-sync.timer enabled"         test -L "$R/etc/systemd/system/timers.target.wants/zpod-rtc-sync.timer"
chk "M4: hwclock installed"                   bash -c "ls '$R'/usr/sbin/hwclock '$R'/sbin/hwclock 2>/dev/null | grep -q ."
chk "M4: fake-hwclock absent"                 bash -c "! ls '$R'/etc/systemd/system/*.wants/fake-hwclock.service '$R'/sbin/fake-hwclock 2>/dev/null | grep -q ."
chk "M4: low-battery guard enabled (M6: zpod-battery)" bash -c "test -L '$R/etc/systemd/system/multi-user.target.wants/zpod-battery-guard.service' || test -L '$R/etc/systemd/system/multi-user.target.wants/zpod-battery.service'"
chk "M4: ui.conf [battery] shutdown_pct = 5"  grep -qx 'shutdown_pct = 5' "$R/etc/zpod/ui.conf"
chk "M4: ui.conf [display] spi_mhz = 32"      grep -qx 'spi_mhz = 32' "$R/etc/zpod/ui.conf"
# --- M4.1: login setup (no password/key baked) + build identity -------------
chk "M4.1: first user has no password hash"    bash -c "awk -F: '\$3==1000{print \$1}' '$R/etc/passwd' | while read u; do f=\$(awk -F: -v u=\"\$u\" '\$1==u{print \$2}' '$R/etc/shadow'); case \"\$f\" in '!'*|'*'*|'') ;; *) exit 1;; esac; done"
chk "M4.1: no authorized_keys in the image"   bash -c "! find '$R/home' '$R/root' -name authorized_keys 2>/dev/null | grep -q ."
chk "M4.1: no key-only sudoers baked"         test ! -e "$R/etc/sudoers.d/010_zpod-nopasswd"
chk "M4.1: boot FAT has no userconf/zpod-ssh.pub" bash -c "! printf '%s\n' \"\$1\" | grep -qE '(userconf(\.txt)?|zpod-ssh\.pub|zpod-user\.txt)\$'" _ "$bootls"
chk "M4.1: zpod-firstuser enabled"            test -L "$R/etc/systemd/system/multi-user.target.wants/zpod-firstuser.service"
chk "M4.1: zpod-firstuser.sh executable"      test -x "$R/usr/lib/zpod/zpod-firstuser.sh"
chk "M4.1: firstuser runs before userconfig"  grep -q '^Before=userconfig.service' "$R/usr/lib/systemd/system/zpod-firstuser.service"
chk "M4.1: userconfig drop-in (skip when set up)" grep -q 'ConditionPathExists=/etc/ssh/sshd_config.d/rename_user.conf' "$R/etc/systemd/system/userconfig.service.d/90-zpod.conf"
chk "M5.1: firstuser also runs for a late userconf.txt" grep -q '^ConditionPathExists=|/boot/firmware/userconf.txt' "$R/usr/lib/systemd/system/zpod-firstuser.service"
chk "M5.1: firstuser applies late userconf.txt"   grep -q '^late_userconf$' "$R/usr/lib/zpod/zpod-firstuser.sh"
chk "M4.1: userconfig still enabled (userconf.txt works)" test -L "$R/etc/systemd/system/multi-user.target.wants/userconfig.service"
chk "M4.1: /etc/zpod-release has git + image" bash -c "grep -q '^ZPOD_GIT=[0-9a-f]\{7\}' '$R/etc/zpod-release' && grep -q '^ZPOD_IMAGE=.' '$R/etc/zpod-release' && grep -q '^ZPOD_VERSION=.' '$R/etc/zpod-release'"
if [[ $M4 = 1 ]]; then
  chk "M4.1: drop server waits for first-user rename" grep -q '^After=.*userconfig.service' "$R/etc/systemd/system/zpod-drop.service"
  for b in zpod-launcher zpod-bootsplash zpod-drop zpod-app; do
    chk "M4: $b is ARMv6"                     bash -c "readelf -A '$R/usr/lib/zpod/bin/$b' | grep -qE 'Tag_CPU_arch: v6'"
  done
  chk "M4: zpod-ui disabled (launcher owns the LCD)" bash -c "! ls '$R'/etc/systemd/system/*.wants/zpod-ui.service 2>/dev/null | grep -q ."
  chk "M4: zpod-bootsplash enabled"           test -L "$R/etc/systemd/system/sysinit.target.wants/zpod-bootsplash.service"
  chk "M4: apps installed (music clock tetris)" bash -c "for a in music clock tetris; do test -x '$R'/opt/zpod/apps/\$a/bin/\$a || exit 1; done"
  chk "M4: drop service runs as uid 1000"     grep -qx 'User=1000' "$R/etc/systemd/system/zpod-drop.service"
else
  echo "info  M4 launcher not in this image (M3-style UI)"
fi

# --- M5: large-card layout, Tools, alarm, file transfer, size trims ----------
M5=0; [[ "$DATA_SIZE" -gt 0 ]] && M5=1
if [[ $M5 = 1 ]]; then
  echo "--- partitions: root ${ROOT_SIZE} B at ${ROOT_START}; data ${DATA_SIZE} B at ${DATA_START}"
  chk "M5: p3 (data) starts after p2 (root)"   test "$DATA_START" -ge $((ROOT_START + ROOT_SIZE))
  chk "M5: root is fixed and >= 3 GiB"          test "$ROOT_SIZE" -ge $((3 * 1024 * 1024 * 1024))
  chk "M5: cmdline has no 'resize' (root is fixed)" bash -c "! grep -qw resize '$TMP/cmdline.txt'"
  chk "M5: fstab /data (nofail, growfs)"       bash -c "grep -E '^PARTUUID=[0-9a-f]{8}-03[[:space:]]+/data[[:space:]]+ext4[[:space:]]+[^[:space:]]*x-systemd.growfs' '$R/etc/fstab' | grep -q nofail"
  chk "M5: fstab bind /data/media -> /srv/zpod/media" grep -qE '^/data/media[[:space:]]+/srv/zpod/media[[:space:]]+none[[:space:]]+bind' "$R/etc/fstab"
  chk "M5: same PARTUUID prefix for p2 and p3" bash -c "[ \"\$(grep -o 'PARTUUID=[0-9a-f]*-02' '$R/etc/fstab' | cut -d- -f1)\" = \"\$(grep -o 'PARTUUID=[0-9a-f]*-03' '$R/etc/fstab' | cut -d- -f1)\" ]"
  chk "M5: /srv/zpod/media on root is an empty mount point" bash -c "[ -d '$R/srv/zpod/media' ] && [ -z \"\$(ls -A '$R/srv/zpod/media')\" ]"
  chk "M5: /data mount point"                   test -d "$R/data"
  DLOOP=$(losetup -f --show -r --offset "$DATA_START" --sizelimit "$DATA_SIZE" "$IMG")
  mkdir -p "$TMP/data"
  dh=$(dumpe2fs -h "$DLOOP" 2>/dev/null)
  chk "M5: data fs label zpoddata"             bash -c "grep -qE '^Filesystem volume name:[[:space:]]+zpoddata' <<<\"\$1\"" _ "$dh"
  rg=$(awk -F: '/^Reserved GDT blocks/{gsub(/ /,"",$2); print $2}' <<<"$dh")
  chk "M5: data fs can grow online to 2 TiB (reserved GDT ${rg:-0} >= 127)" test "${rg:-0}" -ge 127
  chk "M5: data fs 4 KiB blocks, 0 reserved"    bash -c "grep -qE '^Block size:[[:space:]]+4096' <<<\"\$1\" && grep -qE '^Reserved block count:[[:space:]]+0\$' <<<\"\$1\"" _ "$dh"
  mount -o ro,noload -t ext4 "$DLOOP" "$TMP/data"
  D="$TMP/data"
  zuid=$(awk -F: '$1=="zpod"{print $3}' "$R/etc/passwd")
  chk "M5: /data/media/music (uid 1000)"        bash -c "[ \"\$(stat -c %u '$D/media/music')\" = 1000 ]"
  chk "M5: /data/media/playlists group-writable" bash -c "[ \"\$(stat -c %a '$D/media/playlists')\" = 775 ]"
  chk "M5: /data/zpod/appdata owned by zpod"     bash -c "[ \"\$(stat -c %u '$D/zpod/appdata')\" = '$zuid' ]"
  umount "$TMP/data"; losetup -d "$DLOOP"; DLOOP=""
  chk "M5: zpod-data-grow enabled (before local-fs-pre)" test -L "$R/etc/systemd/system/local-fs-pre.target.wants/zpod-data-grow.service"
  chk "M5: zpod-data-setup enabled"            test -L "$R/etc/systemd/system/local-fs.target.wants/zpod-data-setup.service"
  chk "M5: data scripts executable"            bash -c "test -x '$R/usr/lib/zpod/bin/zpod-data-grow' && test -x '$R/usr/lib/zpod/bin/zpod-data-setup'"
  chk "M5: mpd waits for the music bind mount" grep -q '^After=srv-zpod-media.mount' "$R/etc/systemd/system/mpd.service.d/zpod-data.conf"
  chk "M5: launcher ZPOD_DATA_DIR=/data/zpod/appdata" grep -qx 'Environment=ZPOD_DATA_DIR=/data/zpod/appdata' "$R/etc/systemd/system/zpod-launcher.service"
fi
chk "M5: rsync share installed but OFF"        bash -c "test -f '$R/etc/systemd/system/zpod-rsyncd.socket' && ! ls '$R'/etc/systemd/system/*.wants/zpod-rsyncd.socket 2>/dev/null | grep -q ."
chk "M5: rsync share listens on USB only"      grep -qx 'ListenStream=10.12.194.1:873' "$R/etc/systemd/system/zpod-rsyncd.socket"
chk "M5: rsyncd hosts allow = USB subnet"      grep -qx 'hosts allow = 10.12.194.0/28' "$R/etc/zpod/rsyncd.conf"
chk "M5: rsyncd has no secrets file"           bash -c "! grep -qiE '^[[:space:]]*(secrets file|auth users)' '$R/etc/zpod/rsyncd.conf'"
chk "M5: bluetooth.service enabled"            bash -c "ls '$R'/etc/systemd/system/*.wants/bluetooth.service"
for b in iw btmgmt bluetoothctl aplay rsync mpc parted; do
  chk "M5: tool $b" bash -c "ls '$R'/usr/bin/$b '$R'/usr/sbin/$b 2>/dev/null | grep -q ."
done
chk "M5: core-version >= 0.6.0"                bash -c "printf '0.6.0\n%s\n' \"\$(cat '$R/etc/zpod/core-version')\" | sort -V -C"
chk "M5: launcher has Tools (Wi-Fi + Bluetooth scanner)" bash -c "grep -qa 'Wi-Fi scanner' '$R/usr/lib/zpod/bin/zpod-launcher' && grep -qa 'Bluetooth scanner' '$R/usr/lib/zpod/bin/zpod-launcher'"
chk "M5: launcher rings alarms (alarm.conf)"   grep -qa 'alarm.conf' "$R/usr/lib/zpod/bin/zpod-launcher"
chk "M5: launcher file transfer rescan + rsync" bash -c "grep -qa 'zpod-rsyncd.socket' '$R/usr/lib/zpod/bin/zpod-launcher' && grep -qa 'modified-since' '$R/usr/lib/zpod/bin/zpod-launcher'"
chk "M5: clock app 1.1.0 (alarm + timer)"      bash -c "grep -qx 'version = \"1.1.0\"' '$R/opt/zpod/apps/clock/manifest.toml' && grep -qa 'Test: ring in 5 s' '$R/opt/zpod/apps/clock/bin/clock'"
chk "M5: music app 1.1.0 (playlists, A-Z)"     bash -c "grep -qx 'version = \"1.1.0\"' '$R/opt/zpod/apps/music/manifest.toml' && grep -qa 'Shuffle play' '$R/opt/zpod/apps/music/bin/music'"
chk "M5: size trim: dpkg path-exclude"         grep -q '^path-exclude /usr/share/doc/\*' "$R/etc/dpkg/dpkg.cfg.d/90-zpod-trim"
chk "M5: size trim: no man pages"              bash -c "[ -z \"\$(find '$R/usr/share/man' -type f 2>/dev/null | head -1)\" ]"
chk "M5: size trim: copyright files kept"      test -f "$R/usr/share/doc/mpd/copyright"
chk "M5: size trim: no apt lists"              bash -c "[ -z \"\$(find '$R/var/lib/apt/lists' -maxdepth 1 -type f -name '*_Packages*' 2>/dev/null | head -1)\" ]"

# --- M6: battery history, Wi-Fi tools, drop box + OPDS (off), boot trims -----
M6=0; [[ -x "$R/usr/lib/zpod/bin/zpod-battery" ]] && M6=1
if [[ $M6 = 1 ]]; then
  W="$R/etc/systemd/system"
  chk "M6: zpod-release ZPOD_VERSION is m6"     grep -qE '^ZPOD_VERSION=.*m6' "$R/etc/zpod-release"
  chk "M6: core-version 0.8.0"                  grep -qx '0.8.0' "$R/etc/zpod/core-version"
  chk "M6: zpod-battery is ARMv6"               bash -c "readelf -A '$R/usr/lib/zpod/bin/zpod-battery' | grep -qE 'Tag_CPU_arch: v6'"
  chk "M6: zpod-battery enabled"                test -L "$W/multi-user.target.wants/zpod-battery.service"
  chk "M6: Python battery guard disabled"       bash -c "! ls '$W'/*.wants/zpod-battery-guard.service 2>/dev/null | grep -q ."
  chk "M6: zpod-battery after data + RTC"       grep -qx 'After=zpod-data-setup.service zpod-rtc.service' "$W/zpod-battery.service"
  chk "M6: battery log path in binary"          grep -qa 'battery.log' "$R/usr/lib/zpod/bin/zpod-battery"
  chk "M6: drop socket OFF by default"          bash -c "test -f '$W/zpod-drop.socket' && ! ls '$W'/*.wants/zpod-drop.socket 2>/dev/null | grep -q ."
  chk "M6: drop.conf dropbox = off"             grep -qx 'dropbox = off' "$R/etc/zpod/drop.conf"
  chk "M6: drop.conf opds = off"                grep -qx 'opds = off' "$R/etc/zpod/drop.conf"
  chk "M6: drop.conf has no token"              bash -c "! grep -qE '^[[:space:]]*token[[:space:]]*=' '$R/etc/zpod/drop.conf'"
  chk "M6: drop.conf books/inbox on /data"      bash -c "grep -qx 'books_dir = /data/books' '$R/etc/zpod/drop.conf' && grep -qx 'inbox_dir = /data/inbox' '$R/etc/zpod/drop.conf'"
  chk "M6: drop service may write /data/inbox"  grep -qE '^ReadWritePaths=.*-/data/inbox' "$W/zpod-drop.service"
  chk "M6: drop server has OPDS + drop box"     bash -c "grep -qa 'application/atom+xml;profile=opds-catalog' '$R/usr/lib/zpod/bin/zpod-drop' && grep -qa 'META-INF/container.xml' '$R/usr/lib/zpod/bin/zpod-drop' && grep -qa '/inbox/' '$R/usr/lib/zpod/bin/zpod-drop'"
  chk "M6: data setup makes /data/books + /data/inbox" grep -q '/data/books /data/inbox' "$R/usr/lib/zpod/bin/zpod-data-setup"
  for t in 'Book catalog' 'Drop box' 'Hotspot' 'Saved Wi-Fi' 'Channels' 'Signal' 'Full charge lasts' 'boot-notice' 'booted-once' 'zpod-hotspot'; do
    chk "M6: launcher has '$t'" grep -qa "$t" "$R/usr/lib/zpod/bin/zpod-launcher"
  done
  chk "M6: launcher waits for first-boot scripts (notice)" grep -qx 'After=zpod-firstuser.service zpod-wifi-import.service' "$W/zpod-launcher.service"
  chk "M6: firstuser writes boot notices"       bash -c "grep -q '^notice()' '$R/usr/lib/zpod/zpod-firstuser.sh' && grep -q 'notice \"userconf.txt ignored' '$R/usr/lib/zpod/zpod-firstuser.sh'"
  chk "M6: wifi-import writes boot notices"     grep -q 'notice "zpod-wifi.txt: Wi-Fi network added"' "$R/usr/lib/zpod/zpod-wifi-import.sh"
  chk "M6: no hotspot profile shipped"          test ! -e "$R/etc/NetworkManager/system-connections/zpod-hotspot.nmconnection"
  chk "M6: no booted-once marker shipped"       test ! -e "$R/var/lib/zpod/booted-once"
  chk "M6: no Wi-Fi PSK in any shipped profile" bash -c "! grep -rqsE '^(psk|password)=' '$R/etc/NetworkManager/system-connections/'"
  # boot trims
  chk "M6 boot: auto_initramfs=0"               grep -qx 'auto_initramfs=0' "$TMP/config.txt"
  chk "M6 boot: no vc4-kms-v3d"                 bash -c "! grep -q '^dtoverlay=vc4-kms-v3d' '$TMP/config.txt'"
  chk "M6 boot: camera/display autodetect off"  bash -c "grep -qx 'camera_auto_detect=0' '$TMP/config.txt' && grep -qx 'display_auto_detect=0' '$TMP/config.txt'"
  chk "M6 boot: disable_splash=1, boot_delay=0, initial_turbo=30" bash -c "grep -qx 'disable_splash=1' '$TMP/config.txt' && grep -qx 'boot_delay=0' '$TMP/config.txt' && grep -qx 'initial_turbo=30' '$TMP/config.txt'"
  chk "M6 boot: firmware settings in [all] (after the last section header)" bash -c "[ \"\$(grep -E '^\[' '$TMP/config.txt' | tail -1)\" = '[all]' ] && [ \$(grep -n '^disable_splash=1' '$TMP/config.txt' | cut -d: -f1) -gt \$(grep -n '^\[all\]' '$TMP/config.txt' | tail -1 | cut -d: -f1) ]"
  chk "M6 boot: cmdline quiet"                  grep -qw quiet "$TMP/cmdline.txt"
  kv6=$(basename "$R"/lib/modules/*-rpi-v6)
  chk "M6 boot: ext4 + SD host built into $kv6 (no initramfs needed)" bash -c "for m in kernel/fs/ext4/ext4.ko kernel/drivers/mmc/core/mmc_block.ko kernel/drivers/mmc/host/bcm2835.ko; do grep -qx \$m '$R/lib/modules/$kv6/modules.builtin' || exit 1; done"
  chk "M6 boot: cmdline rootfstype=ext4 + rootwait" bash -c "grep -qw 'rootfstype=ext4' '$TMP/cmdline.txt' && grep -qw rootwait '$TMP/cmdline.txt'"
  chk "M6 boot: keyboard-setup masked"          bash -c "[ \"\$(readlink '$W/keyboard-setup.service')\" = /dev/null ]"
  chk "M6 boot: rpi-eeprom-update masked"       bash -c "[ \"\$(readlink '$W/rpi-eeprom-update.service')\" = /dev/null ]"
  chk "M6 boot: apparmor + e2scrub_reap off"    bash -c "! ls '$W'/*.wants/apparmor.service '$W'/*.wants/e2scrub_reap.service 2>/dev/null | grep -q ."
  for u in ssh zpod-usb-gadget rpi-usb-gadget-ics zpod-firstuser zpod-wifi-import zpod-data-grow zpod-data-setup userconfig regenerate_ssh_host_keys; do
    chk "M6 boot: still enabled: $u" bash -c "ls '$W'/*.wants/$u.service 2>/dev/null | grep -q ."
  done
fi

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
for m in ('zpod_ui', 'zpod_buttons', 'zpod_battery_guard'):
    spec = importlib.util.spec_from_file_location(m, '/usr/lib/zpod/%s.py' % m)
    mod = importlib.util.module_from_spec(spec); spec.loader.exec_module(mod)
img = PIL.Image.new('RGB', (320, 240), (255, 0, 0))
b = zpod_lcd.to_rgb565(img)
assert len(b) == 153600 and b[:2] == bytes([0xF8, 0x00]), b[:2]
print('imports ok; PIL', PIL.__version__, '; evdev KEY_ENTER', e.KEY_ENTER)
" 2>&1) && ok "chroot python: $out" || bad "chroot python: $out"
  out=$(chroot "$MNT" /usr/bin/env PYTHONDONTWRITEBYTECODE=1 python3 -c "
import sys, importlib.util
sys.path.insert(0, '/usr/lib/zpod')
spec = importlib.util.spec_from_file_location('zpod_ui', '/usr/lib/zpod/zpod_ui.py')
u = importlib.util.module_from_spec(spec); spec.loader.exec_module(u)
class FakeLCD:
    def __init__(s): s.log = []
    def show(s, img): s.log.append(('show', img.size, img.getpixel((10, 10))))
    def blank_off(s): s.log.append(('blank_off',))
assert u.shutdown_screen_mode() == 'message', u.shutdown_screen_mode()
r = {}
for kind, mode in (('poweroff', 'message'), ('poweroff', 'off'), ('reboot', 'message'), (None, 'message')):
    f = FakeLCD(); u.end_screen(f, kind, mode); r[(kind, mode)] = f.log[0][0]
assert r == {('poweroff','message'):'show', ('poweroff','off'):'blank_off', ('reboot','message'):'show', (None,'message'):'blank_off'}, r
img = u.final_image('poweroff'); assert img.size == (320, 240)
print('end_screen ok', sorted((str(k), v) for k, v in r.items()))
" 2>&1) && ok "M3 chroot: shutdown screen logic ($out)" || bad "M3 chroot: shutdown screen logic: $out"
  # M3.1: run the first-boot Wi-Fi import against a fake boot dir in /tmp
  out=$(chroot "$MNT" /bin/bash -c '
set -e
B=/tmp/zw-boot C=/tmp/zw-conn; rm -rf $B $C; mkdir -p $B
printf "\xef\xbb\xbfSSID=ZPOD Test;Net\r\nPSK=p\\\\ss w0rd!x\r\n" > $B/zpod-wifi.txt
printf "[connection]\nid=extra\ntype=wifi\n\n[wifi]\nssid=Other\n" > $B/extra.nmconnection
ZPOD_BOOT_DIR=$B ZPOD_CONN_DIR=$C ZPOD_NO_COUNTRY=1 /usr/lib/zpod/zpod-wifi-import.sh >/dev/null
f=$C/zpod-wifi.nmconnection
test ! -e $B/zpod-wifi.txt; test ! -e $B/extra.nmconnection
test "$(stat -c %a $f)" = 600; test "$(stat -c %a $C/extra.nmconnection)" = 600
grep -qx "ssid=ZPOD Test;Net" $f; grep -qx "psk=p\\\\\\\\ss w0rd!x" $f
grep -qx "powersave=2" $f; grep -qx "key-mgmt=wpa-psk" $f; grep -qx "powersave=2" $C/extra.nmconnection
rm -rf $B $C; echo import ok' 2>&1) && ok "M3.1 chroot: zpod-wifi-import (txt + nmconnection, 600, powersave=2, source deleted)" || bad "M3.1 chroot: zpod-wifi-import: $out"
  if [[ $M6 = 1 ]]; then
    out=$(chroot "$MNT" /bin/bash -c '
set -e
B=/tmp/zn-boot C=/tmp/zn-conn N=/tmp/zn-notice O=/tmp/zn-once; rm -rf $B $C $N $O; mkdir -p $B
printf "SSID=Late Net\nPSK=latepassword1\n" > $B/zpod-wifi.txt
ZPOD_BOOT_DIR=$B ZPOD_CONN_DIR=$C ZPOD_NO_COUNTRY=1 ZPOD_BOOTED_ONCE=$O ZPOD_NOTICE=$N /usr/lib/zpod/zpod-wifi-import.sh >/dev/null
test ! -e $N
touch $O; printf "SSID=Late Net\nPSK=latepassword1\n" > $B/zpod-wifi.txt
ZPOD_BOOT_DIR=$B ZPOD_CONN_DIR=$C ZPOD_NO_COUNTRY=1 ZPOD_BOOTED_ONCE=$O ZPOD_NOTICE=$N /usr/lib/zpod/zpod-wifi-import.sh >/dev/null
grep -qx "zpod-wifi.txt: Wi-Fi network added" $N; ! grep -q "latepassword1\|Late Net" $N
rm -rf $B $C $N $O; echo notice ok' 2>&1) && ok "M6 chroot: late zpod-wifi.txt -> boot notice (no SSID/PSK in it; none on first boot)" || bad "M6 chroot: wifi-import notice: $out"
    out=$(chroot "$MNT" /usr/bin/env ZPOD_I2C=/tmp/no-such-i2c /usr/lib/zpod/bin/zpod-battery --once 2>&1; echo "rc=$?")
    [[ "$out" == *"no MAX17048"*"rc=1"* ]] && ok "M6 chroot: zpod-battery runs (ARMv6/qemu), reports a missing gauge cleanly" || bad "M6 chroot: zpod-battery --once: $out"
  fi
  if [[ $M5 = 1 ]]; then
    # zpod-data-grow arithmetic on fake sysfs: 128 GB card, 4 TB card (2 TiB cap), already grown
    out=$(chroot "$MNT" /bin/sh -c '
set -e
t() { d=/tmp/zg; rm -rf $d; mkdir -p $d/mmcblk0p3; echo $1 > $d/size; echo 7471104 > $d/mmcblk0p3/start; echo $2 > $d/mmcblk0p3/size
      ZPOD_SYS_BLOCK=$d ZPOD_DRY_RUN=1 /usr/lib/zpod/bin/zpod-data-grow; }
t 250069680 524288 | grep -q -e "-> 250069679"
t 7814037168 524288 | grep -q -e "-> 4294967295"
test -z "$(t 8000000 528000)"
rm -rf /tmp/zg; echo grow ok' 2>&1) && ok "M5 chroot: zpod-data-grow targets (128 GB card, 4 TB -> 2 TiB cap, no-op when grown)" || bad "M5 chroot: zpod-data-grow: $out"
  fi
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
