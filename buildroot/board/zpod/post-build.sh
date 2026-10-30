#!/bin/sh
# ZPOD post-build: read-only squashfs rootfs plumbing. Everything writable is
# on /data (p4) or tmpfs. Runs with TARGET_DIR set by Buildroot.
set -eu
T="${TARGET_DIR}"
BOARD_DIR="$(dirname "$0")"
mkdir -p "$T/data" "$T/boot/firmware" "$T/opt/zpod" "$T/srv/zpod" "$T/var/lib"
# Apps, media and state live on the data partition
ln -sfn /data/zpod/apps "$T/opt/zpod/apps"
# /srv/zpod/media is a directory: /data/media is bind-mounted on it (zpod-data-early)
rm -rf "$T/srv/zpod/media"; mkdir -p "$T/srv/zpod/media"
for d in mpd zpod alsa seedrng dbus bluetooth; do
	rm -rf "$T/var/lib/$d"; ln -sfn "/data/var/$d" "$T/var/lib/$d"
done
# root's ~/.ssh on /data (dropbear reads ~/.ssh/authorized_keys)
mkdir -p "$T/root"; chmod 700 "$T/root"
rm -rf "$T/root/.ssh"; ln -sfn /data/etc/ssh "$T/root/.ssh"
# Drop init scripts we do not want on a handheld. S40network (ifup lo) is
# replaced by "ip link set lo up" in S00zpod-data; D-Bus and bluetoothd are
# started in the background by S45bluetooth.
rm -f "$T/etc/init.d/S50telnet" "$T/etc/init.d/S90httpd" "$T/etc/init.d/S41ifplugd" \
	"$T/etc/init.d/S50crond" "$T/etc/init.d/S40network" "$T/etc/init.d/S30dbus" "$T/etc/init.d/S30dbus-daemon" \
	"$T/etc/init.d/S40bluetoothd" "$T/etc/init.d/S40bluetooth"
# Dropbear host keys persist on /data
ln -sfn /data/etc/dropbear "$T/etc/dropbear"
# Accounts move to /data in post-fakeroot.sh (after the users tables are applied)
# D-Bus machine id on /data
ln -sfn /data/etc/machine-id "$T/etc/machine-id"
# sudo: group sudo may sudo with its password; key-only users get a
# NOPASSWD drop-in on /data (zpod-firstuser-br)
if [ -f "$T/etc/sudoers" ] && ! grep -q '^@includedir /data/etc/sudoers.d' "$T/etc/sudoers"; then
	printf '\n# ZPOD\n%%sudo ALL=(ALL:ALL) ALL\n@includedir /data/etc/sudoers.d\n' >> "$T/etc/sudoers"
fi
# btmgmt (the Tools > Bluetooth scanner's first choice) is built by BlueZ but
# not installed (noinst); install it from the build tree
for b in "${BUILD_DIR}"/bluez5_utils-*/tools/btmgmt; do
	[ -x "$b" ] && install -m 0755 "$b" "$T/usr/bin/btmgmt"
done
# rsync share (same config as Pi OS); the unit file is a marker for the
# launcher's File transfer screen (the systemctl shim drives S61zpod-rsyncd)
install -D -m 0644 "${BR2_EXTERNAL_ZPOD_PATH}/../apps/system/etc/zpod/rsyncd.conf" "$T/etc/zpod/rsyncd.conf"
install -D -m 0644 "${BR2_EXTERNAL_ZPOD_PATH}/../apps/system/systemd/zpod-rsyncd.socket" "$T/usr/lib/systemd/system/zpod-rsyncd.socket"
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
