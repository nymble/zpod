#!/bin/sh
# ZPOD post-fakeroot (runs after Buildroot has applied the users tables and
# before the squashfs is made; TARGET_DIR is the file-system copy).
# Accounts live on /data (read-only root): keep the image's files as the base
# in /usr/share/zpod/etc-base and make /etc/{passwd,group,shadow} symlinks.
# zpod-data-early seeds /data/etc from the base at boot. Root has no password
# (locked: SSH is key-only for root); the login user (uid 1000) comes from
# userconf.txt / zpod-user.txt at first boot (zpod-firstuser-br).
set -eu
T="$1"
grep -q '^sudo:' "$T/etc/group" || echo 'sudo:x:27:' >> "$T/etc/group"
sed -i 's/^root:[^:]*:/root:*:/' "$T/etc/shadow"
mkdir -p "$T/usr/share/zpod/etc-base"
for f in passwd group shadow; do
	if [ ! -L "$T/etc/$f" ]; then
		cp -p "$T/etc/$f" "$T/usr/share/zpod/etc-base/$f"
		rm -f "$T/etc/$f"
	fi
	ln -sfn "/data/etc/$f" "$T/etc/$f"
done
chmod 600 "$T/usr/share/zpod/etc-base/shadow"
echo "post-fakeroot: accounts -> /data/etc ($(cut -d: -f1 "$T/usr/share/zpod/etc-base/passwd" | tr '\n' ' '))"
