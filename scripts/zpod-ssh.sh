# Sourced by the ZPOD Mac scripts (zpod-app-push.sh, zpod-dev): one ssh setup.
#   ZPOD_HOST  device address (default 10.12.194.1, the USB CDC-NCM link)
#   ZPOD_USER  login (default root: Buildroot images with zpod-ssh.pub; on the
#              Pi OS image use your login user, e.g. pi; commands then use sudo)
#   ZPOD_SSH_OPTS  extra ssh options
# On the USB link (10.12.194.x, a point-to-point cable) host keys are not
# pinned, because every reflash makes new ones. Other hosts use normal
# known_hosts checking. A shared connection (ControlMaster) makes the second
# and later commands start in a few ms instead of a new handshake.
ZPOD_HOST=${ZPOD_HOST:-10.12.194.1}
ZPOD_USER=${ZPOD_USER:-root}
zpod_ssh_opts() {
	o="-o ConnectTimeout=5 -o ControlMaster=auto -o ControlPath=/tmp/zpod-ssh-%r@%h:%p -o ControlPersist=120"
	case "$ZPOD_HOST" in
	10.12.194.*) o="$o -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR" ;;
	esac
	echo "$o ${ZPOD_SSH_OPTS:-}"
}
# zpod_ssh [-t] 'remote command'
zpod_ssh() {
	# shellcheck disable=SC2046
	ssh $(zpod_ssh_opts) "$@"
}
# Remote commands start with this: dropbear's default PATH has no /usr/local/bin.
ZPOD_RPATH='export PATH=/usr/local/bin:/usr/sbin:/sbin:$PATH;'
zpod_target() { echo "$ZPOD_USER@$ZPOD_HOST"; }
# "sudo " unless we log in as root
zpod_sudo() { [ "$ZPOD_USER" = root ] || echo "sudo "; }
