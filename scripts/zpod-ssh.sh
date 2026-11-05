# Sourced by the ZPOD Mac scripts (zpod-app-push.sh, zpod-dev): one ssh setup.
#   ZPOD_HOST  device address. Unset: first reachable (ssh port answers within
#              about 1 s) of: the address saved in ~/.zpod-host (ZPOD_HOST_FILE),
#              10.12.194.1 (USB CDC-NCM link), zpod.local (mDNS, Pi OS image),
#              zpod (router DNS from the DHCP hostname). Nothing reachable:
#              10.12.194.1, so the error names the USB link.
#              Save the Wi-Fi address once with: scripts/zpod-dev --save-host
#   ZPOD_USER  login (default root: Buildroot images with zpod-ssh.pub; on the
#              Pi OS image use your login user, e.g. pi; commands then use sudo)
#   ZPOD_SSH_OPTS  extra ssh options
# On the USB link (10.12.194.x, a point-to-point cable) host keys are not
# pinned, because every reflash makes new ones. Other hosts (Wi-Fi) use normal
# known_hosts checking (the Buildroot host key lives on /data, so it survives
# reboots; a reflash that wipes /data makes a new one). A shared connection
# (ControlMaster) makes the second and later commands start in a few ms
# instead of a new handshake.
ZPOD_HOST=${ZPOD_HOST:-}
ZPOD_USER=${ZPOD_USER:-root}
ZPOD_HOST_FILE=${ZPOD_HOST_FILE:-$HOME/.zpod-host}
ZPOD_USB_HOST=10.12.194.1
# zpod_port_open host: does something answer on port 22 within ~1 s?
zpod_port_open() {
	if nc -h 2>&1 | grep -q -- ' -G '; then
		nc -z -G 1 -w 1 "$1" 22 >/dev/null 2>&1   # macOS nc: -G = connect timeout
	else
		nc -z -w 1 "$1" 22 >/dev/null 2>&1
	fi
}
zpod_saved_host() {
	[ -r "$ZPOD_HOST_FILE" ] && sed -n 's/[[:space:]]*#.*//; /[^[:space:]]/{s/[[:space:]]//g;p;q;}' "$ZPOD_HOST_FILE"
}
# Sets ZPOD_HOST once (explicit -H / ZPOD_HOST always wins, no probing).
zpod_resolve_host() {
	[ -n "$ZPOD_HOST" ] && return 0
	saved=$(zpod_saved_host || true)
	for h in $saved $ZPOD_USB_HOST zpod.local zpod; do
		if zpod_port_open "$h"; then ZPOD_HOST=$h; break; fi
	done
	ZPOD_HOST=${ZPOD_HOST:-$ZPOD_USB_HOST}
	[ "$ZPOD_HOST" = "$ZPOD_USB_HOST" ] || echo "zpod: using $ZPOD_HOST" >&2
	export ZPOD_HOST
}
zpod_ssh_opts() {
	o="-o ConnectTimeout=5 -o ControlMaster=auto -o ControlPath=/tmp/zpod-ssh-%r@%h:%p -o ControlPersist=120"
	case "$ZPOD_HOST" in
	10.12.194.*) o="$o -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR" ;;
	esac
	echo "$o ${ZPOD_SSH_OPTS:-}"
}
# zpod_ssh [-t] 'remote command'
zpod_ssh() {
	zpod_resolve_host
	# shellcheck disable=SC2046
	ssh $(zpod_ssh_opts) "$@"
}
# Remote commands start with this: dropbear's default PATH has no /usr/local/bin.
ZPOD_RPATH='export PATH=/usr/local/bin:/usr/sbin:/sbin:$PATH;'
zpod_target() { zpod_resolve_host; echo "$ZPOD_USER@$ZPOD_HOST"; }
# "sudo " unless we log in as root
zpod_sudo() { [ "$ZPOD_USER" = root ] || echo "sudo "; }
# The device's Wi-Fi (wlan0) IPv4 address, asked over the current link.
zpod_wifi_ip() {
	zpod_ssh "$(zpod_target)" "ip -4 -o addr show wlan0 2>/dev/null" | sed -n 's/.* inet \([0-9.]*\)\/.*/\1/p' | head -1
}
