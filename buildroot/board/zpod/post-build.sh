#!/bin/sh
# ZPOD post-build: read-only squashfs rootfs plumbing. Everything writable is
# on /data (p4) or tmpfs. Runs with TARGET_DIR set by Buildroot.
set -eu
T="${TARGET_DIR}"
BOARD_DIR="$(dirname "$0")"
mkdir -p "$T/data" "$T/boot/firmware" "$T/opt/zpod" "$T/srv/zpod" "$T/var/lib"
# Apps, media and state live on the data partition
ln -sfn /data/zpod/apps "$T/opt/zpod/apps"
ln -sfn /data/media "$T/srv/zpod/media"
for d in mpd zpod alsa seedrng; do
	rm -rf "$T/var/lib/$d"; ln -sfn "/data/var/$d" "$T/var/lib/$d"
done
# root's ~/.ssh on /data (dropbear reads ~/.ssh/authorized_keys)
mkdir -p "$T/root"; chmod 700 "$T/root"
rm -rf "$T/root/.ssh"; ln -sfn /data/etc/ssh "$T/root/.ssh"
# Drop init scripts we do not want on a handheld
rm -f "$T/etc/init.d/S50telnet" "$T/etc/init.d/S90httpd" "$T/etc/init.d/S41ifplugd" "$T/etc/init.d/S50crond"
# mdev: device access for the "zpod" app user (LCD, GPIO, I2C, buttons)
if ! grep -q '^# ZPOD' "$T/etc/mdev.conf" 2>/dev/null; then
	{
		echo "# ZPOD: LCD (spidev), GPIO (cdev + gpiomem), I2C gauge, uinput buttons"
		echo 'spidev[0-9].[0-9]	root:zpod 660'
		echo 'gpiochip[0-9]*	root:zpod 660'
		echo 'gpiomem		root:zpod 660'
		echo 'i2c-[0-9]*	root:zpod 660'
		echo 'input/event[0-9]+	root:input 660'
		grep -v '^input/event' "$T/etc/mdev.conf"
	} > "$T/etc/mdev.conf.new"
	mv "$T/etc/mdev.conf.new" "$T/etc/mdev.conf"
fi
# Build identification (the launcher's About / Storage & build screens)
REPO="${BR2_EXTERNAL_ZPOD_PATH}/.."
DESC=$(git -C "$REPO" describe --always --dirty --tags 2>/dev/null || echo unknown)
cat > "$T/etc/zpod-release" <<EOR
ZPOD_BUILD=buildroot
ZPOD_VERSION=$(cat "$T/etc/zpod/core-version" 2>/dev/null || echo 0)
ZPOD_GIT=$DESC
ZPOD_BUILD_DATE=$(date -u +%Y-%m-%d)
EOR
