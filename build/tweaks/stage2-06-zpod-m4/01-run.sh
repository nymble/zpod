#!/bin/bash -e
# DRAFT (M4, not yet wired into build/tweaks/apply.sh): install the Rust
# launcher, boot splash, drop server, zpod-app and the bundled apps.
# Expects apps/dist from apps/tools/build-arm.sh (run on the build host first).
M4="${BASE_DIR}/../apps"          # adjust to where apply.sh copies the repo
DIST="${M4}/dist"
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
install -d -o pi -g pi -m 0775 /srv/zpod/media /srv/zpod/media/music /srv/zpod/media/playlists
usermod -aG pi mpd || true     # mpd reads the music dir via group
chmod 0775 /srv/zpod/media/playlists && chown pi:mpd /srv/zpod/media/playlists || true
for a in /tmp/zpod-m4-packages/*.zpa; do [ -e "$a" ] && ZPOD_CORE_VERSION_FILE=/etc/zpod/core-version /usr/lib/zpod/bin/zpod-app install --force "$a"; done
systemctl disable zpod-ui.service || true
systemctl enable zpod-bootsplash.service zpod-launcher.service zpod-drop.socket mpd.service
systemctl disable mpd.socket 2>/dev/null || true
CHEOF
rm -rf "${ROOTFS_DIR}/tmp/zpod-m4-packages"
