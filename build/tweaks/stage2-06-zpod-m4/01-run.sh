#!/bin/bash -e
# ZPOD M4 (installed by build/tweaks/apply.sh as stage2/06-zpod-m4): the Rust
# launcher, boot splash, drop server, zpod-app and the bundled apps.
# files/dist comes from apps/tools/build-arm.sh, files/system from apps/system.
M4="files"
DIST="files/dist"
[ -d "${DIST}/core" ] || { echo "stage2-06-zpod-m4: run apps/tools/build-arm.sh first" >&2; exit 1; }

install -d "${ROOTFS_DIR}/usr/lib/zpod/bin" "${ROOTFS_DIR}/opt/zpod/apps" "${ROOTFS_DIR}/etc/zpod"
install -m 0755 "${DIST}"/core/usr/lib/zpod/bin/* "${ROOTFS_DIR}/usr/lib/zpod/bin/"
ln -sf /usr/lib/zpod/bin/zpod-app "${ROOTFS_DIR}/usr/local/bin/zpod-app"
ln -sf /usr/lib/zpod/bin/zpod-bootsplash "${ROOTFS_DIR}/usr/local/bin/zpod-bootsplash"
install -m 0644 "${M4}"/system/systemd/*.service "${M4}"/system/systemd/*.socket "${ROOTFS_DIR}/etc/systemd/system/"
install -m 0644 "${M4}/system/etc/zpod/core-version" "${ROOTFS_DIR}/etc/zpod/core-version"
[ -f "${ROOTFS_DIR}/etc/zpod/drop.conf" ] || install -m 0644 "${M4}/system/etc/zpod/drop.conf" "${ROOTFS_DIR}/etc/zpod/drop.conf"
[ -f "${ROOTFS_DIR}/etc/mpd.conf.dist" ] || cp "${ROOTFS_DIR}/etc/mpd.conf" "${ROOTFS_DIR}/etc/mpd.conf.dist" 2>/dev/null || true
install -m 0644 "${M4}/system/etc/mpd/mpd.conf" "${ROOTFS_DIR}/etc/mpd.conf"
# /etc/zpod/ui.conf (M3) is kept as-is: the launcher reads [shutdown] screen=.

install -d "${ROOTFS_DIR}/tmp/zpod-m4-packages"
cp "${DIST}"/packages/*.zpa "${ROOTFS_DIR}/tmp/zpod-m4-packages/"

on_chroot << 'CHEOF'
set -e
# Unprivileged app user: LCD (spi, gpio), buttons (input), gauge/RTC (i2c), audio.
id zpod >/dev/null 2>&1 || useradd --system --home-dir /var/lib/zpod/home --create-home \
    --shell /usr/sbin/nologin zpod
for g in spi gpio i2c input audio video; do getent group "$g" >/dev/null && usermod -aG "$g" zpod; done
# The first user is created as "pi" (uid 1000) and renamed at first boot
# (userconf.txt / Imager, e.g. to "paul"), so use the uid, never the name.
U=$(getent passwd 1000 | cut -d: -f1)
install -d -o 1000 -g 1000 -m 0775 /srv/zpod/media /srv/zpod/media/music /srv/zpod/media/playlists
usermod -aG "$U" mpd || true   # mpd reads the music dir via the user's group
chown 1000:mpd /srv/zpod/media/playlists || true
for a in /tmp/zpod-m4-packages/*.zpa; do [ -e "$a" ] && ZPOD_CORE_VERSION_FILE=/etc/zpod/core-version /usr/lib/zpod/bin/zpod-app install --force "$a"; done
systemctl disable zpod-ui.service || true
systemctl enable zpod-bootsplash.service zpod-launcher.service zpod-drop.socket mpd.service
systemctl disable mpd.socket 2>/dev/null || true
CHEOF
rm -rf "${ROOTFS_DIR}/tmp/zpod-m4-packages"
