#!/bin/bash -e
# ZPOD M5 (installed by build/tweaks/apply.sh as stage2/07-zpod-m5):
# - large-card layout support (/data partition grow + setup units; the
#   partition itself is made by scripts/export-image-offset.sh)
# - optional rsync share (installed, disabled)
# - Bluetooth for the Tools scanner (bluez is in stage2-01; enable the daemon)
# - image size trims (docs, man pages, translations, apt lists)
# files/system = apps/system.
S="files/system"

install -d "${ROOTFS_DIR}/usr/lib/zpod/bin" "${ROOTFS_DIR}/etc/zpod" "${ROOTFS_DIR}/data" \
	"${ROOTFS_DIR}/etc/systemd/system/mpd.service.d" "${ROOTFS_DIR}/etc/dpkg/dpkg.cfg.d"
install -m 0755 "$S"/sbin/zpod-data-grow "$S"/sbin/zpod-data-setup "${ROOTFS_DIR}/usr/lib/zpod/bin/"
for u in zpod-data-grow.service zpod-data-setup.service zpod-rsyncd.socket zpod-rsyncd@.service \
	zpod-launcher.service zpod-drop.service; do
	install -m 0644 "$S/systemd/$u" "${ROOTFS_DIR}/etc/systemd/system/$u"
done
install -m 0644 "$S/systemd/mpd.service.d/zpod-data.conf" "${ROOTFS_DIR}/etc/systemd/system/mpd.service.d/zpod-data.conf"
install -m 0644 "$S/etc/zpod/rsyncd.conf" "${ROOTFS_DIR}/etc/zpod/rsyncd.conf"
install -m 0644 "$S/etc/zpod/core-version" "${ROOTFS_DIR}/etc/zpod/core-version"
install -m 0644 "$S/etc/dpkg/90-zpod-trim" "${ROOTFS_DIR}/etc/dpkg/dpkg.cfg.d/90-zpod-trim"

on_chroot << 'CHEOF'
set -e
systemctl enable zpod-data-grow.service zpod-data-setup.service
# rsync share: installed but OFF (File transfer screen / systemctl turns it on)
systemctl disable zpod-rsyncd.socket 2>/dev/null || true
# Debian's own rsync daemon stays off too
systemctl disable rsync.service 2>/dev/null || true
# Bluetooth daemon for bluetoothctl (the scanner uses btmgmt as root first)
systemctl enable bluetooth.service 2>/dev/null || true
# Size trims: the dpkg filter above keeps them out of future installs too
find /usr/share/doc -mindepth 1 ! -name copyright ! -type d -delete 2>/dev/null || true
find /usr/share/doc -mindepth 1 -type d -empty -delete 2>/dev/null || true
rm -rf /usr/share/man/* /usr/share/info/* /usr/share/help/* 2>/dev/null || true
find /usr/share/locale -mindepth 1 -maxdepth 1 ! -name 'en*' ! -name locale.alias -exec rm -rf {} + 2>/dev/null || true
apt-get clean
rm -rf /var/lib/apt/lists/* /var/cache/apt/*.bin
# man-db's daily job has nothing to index now
systemctl disable man-db.timer 2>/dev/null || true
# sanity: the DAC module and the tools the M5 features need are still there
ls /lib/modules/*-rpi-v6/updates/dkms/aoide-zpod-dac.ko* >/dev/null
for b in /usr/sbin/iw /usr/bin/btmgmt /usr/bin/bluetoothctl /usr/bin/aplay /usr/bin/rsync /usr/bin/mpc /usr/sbin/parted; do
	test -x "$b" || { echo "stage2-07-zpod-m5: missing $b" >&2; exit 1; }
done
CHEOF
