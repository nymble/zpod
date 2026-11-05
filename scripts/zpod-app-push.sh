#!/bin/sh
# Install or update ZPOD app packages (.zpa) from a Mac over USB (or Wi-Fi).
#   scripts/zpod-app-push.sh [-H host] [-u user] [--restart] app-1.2.0.zpa [more.zpa...]
#   -H|--host h default $ZPOD_HOST, else ~/.zpod-host / USB 10.12.194.1 /
#               zpod.local / zpod, first that answers (scripts/zpod-ssh.sh)
#   -u user     default $ZPOD_USER or root (Buildroot). Pi OS: -u pi (sudo asks
#               for the password; nothing is stored)
#   --restart   then show the (last) app on the screen: restart it if it runs,
#               launch it otherwise (needs a launcher with the control socket)
# Packages go over the ssh connection as a tar stream (no scp/rsync needed on
# the device) and are installed with "zpod-app install --force", which
# installs, updates or reinstalls the same version (the dev loop pushes the
# same version again and again).
set -eu
here=$(cd "$(dirname "$0")" && pwd)
. "$here/zpod-ssh.sh"
RESTART=""
while [ $# -gt 0 ]; do
	case "$1" in
	-H|--host) ZPOD_HOST=$2; shift 2 ;;
	--host=*) ZPOD_HOST=${1#--host=}; shift ;;
	-u) ZPOD_USER=$2; shift 2 ;;
	--restart) RESTART=1; shift ;;
	--force) shift ;; # always forced (kept for old command lines)
	-h|--help) sed -n '2,14p' "$0"; exit 0 ;;
	-*) echo "unknown option $1" >&2; exit 2 ;;
	*) break ;;
	esac
done
[ $# -ge 1 ] || { sed -n '2,14p' "$0"; exit 2; }
for p in "$@"; do [ -f "$p" ] || { echo "$p: no such file" >&2; exit 1; }; done

S=$(zpod_sudo)
tmp=/tmp/zpod-push-$$
cmds="$ZPOD_RPATH set -e;"
for p in "$@"; do
	b=$(basename "$p")
	cmds="$cmds ${S}zpod-app install --force $tmp/$b; id=\$(zpod-app info $tmp/$b | sed -n 's/^id: *//p');"
done
cmds="$cmds rm -rf $tmp;"
[ -z "$RESTART" ] || cmds="$cmds ${S}zpod-app restart \$id;"

# All packages in one tar stream. COPYFILE_DISABLE: no macOS ._ files;
# ustar: BusyBox tar reads it without pax warnings.
stage=$(mktemp -d "${TMPDIR:-/tmp}/zpod-push.XXXXXX")
trap 'rm -rf "$stage"' EXIT
cp "$@" "$stage/"
tarball() {
	( cd "$stage" && COPYFILE_DISABLE=1 tar --format ustar -cf - ./*.zpa )
}
if [ -z "$S" ]; then
	# root: upload and install in one round trip
	tarball | zpod_ssh "$(zpod_target)" "mkdir -p $tmp && tar -xf - -C $tmp && $cmds"
else
	# sudo may ask for a password, which needs a terminal: upload first, then install
	tarball | zpod_ssh "$(zpod_target)" "mkdir -p $tmp && tar -xf - -C $tmp"
	zpod_ssh -t "$(zpod_target)" "$cmds"
fi
